#!/usr/bin/env python3
"""One-command autonomous Godot ZoneSpec build orchestrator.

It is intentionally the only ordered entry point a model needs to call after a
reviewed concept batch exists.  Every derived artifact is regenerated from the
canonical annotations: terrain, asset catalog/plan, Godot adapter, candidate
scene, render capture, and acceptance reports. Dormant cross-engine manifests
are opt-in. It never treats a stale manifest as evidence of a new build.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any, Iterable

from asset_catalog import build_catalog
from asset_plan import resolve_asset_plan
from asset_physical_acceptance import evaluate as evaluate_asset_physical
from boundary_plan import write as write_boundary_plan
from collision_plan import build as build_collision_plan
from navigation_plan import write as write_navigation_plan
from silhouette import from_batch as measure_silhouette
from collision_acceptance import evaluate as evaluate_collision
from navmesh_acceptance import evaluate as evaluate_navmesh
from asset_visual_preflight import preflight as preflight_assets
from evidence_overlay import render_overlay
from material_catalog import resolve_terrain_materials
from terrain_material_bake import bake_material_preview
from navigation_acceptance import evaluate_navigation
from overview_projection_acceptance import evaluate_projection
from perspective_acceptance import evaluate_perspective
from style_calibration import initial as initial_style_calibration
from style_calibration import refine as refine_style_calibration
from style_calibration import select_best_pass
from style_reference import profile as style_profile, score as style_score
from traversal_probe import evaluate_traversal
from visual_acceptance import evaluate_visual, _image
from worldbuilder_dsl import apply_intent, compile_intent
from zone_acceptance import evaluate_zone
from zone_assets_to_godot import adapt_asset_plan
from zone_compiler import ZoneCompileError, compile_file
from zone_rasterizer import rasterize_zone_spec, write_raster
from zone_runtime_effects import build_runtime_effects
BUILD_VERSION = "codeweald.zone-build/v1"


def _engine_root() -> Path:
    """Where WGE's own Julia and Rust components live.

    Found by walking up from this file until a directory holds both, rather
    than derived from the *asset* root as `project_root.parent`. That old form
    encoded "the engine is exactly one level above the art", which is only true
    while the compiler lives inside the game it compiles for -- the assumption
    that had to go before WGE could be separated from Codeweald (D8).

    Deriving it from `__file__` is also simply more correct: the engine knows
    where it is. It should never have been inferring its own location from
    where somebody else's assets happen to sit.
    """
    for candidate in Path(__file__).resolve().parents:
        if (candidate / "terrain_lab").is_dir() and (candidate / "world_core").is_dir():
            return candidate
    raise ZoneCompileError(
        "cannot locate the WGE engine root: no ancestor of %s contains both "
        "terrain_lab/ and world_core/" % Path(__file__).resolve()
    )


def _write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _canonical_sha256(value: Any) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _run(command: list[str], project_root: Path) -> None:
    result = subprocess.run(command, cwd=project_root, text=True, capture_output=True)
    if result.stdout:
        print(result.stdout, end="")
    if result.returncode != 0:
        # Julia's gates report their numbers on stdout and exit non-zero with
        # an empty stderr, so raising on stderr alone turned a precise
        # "accessible p99=2.06 max=10.21" into a bare "exit 1" for every
        # caller that only sees the exception.
        detail = result.stderr.strip()
        if not detail and result.stdout.strip():
            detail = result.stdout.strip().splitlines()[-1]
        if not detail:
            detail = "exit %d" % result.returncode
        raise ZoneCompileError("Command failed: %s" % detail)


def _accessibility_diagnosis(report_path: Path, zone_spec_path: Path) -> str:
    """Translate a failed accessibility gate into an authoring repair.

    The gate reports slope percentiles, which name neither the knob that
    caused them nor the direction to move it. An author who set a value the
    DSL documents as legal -- `cross_jitter` is documented 0.0-0.5 and 0.5
    fails this gate outright -- gets a number about grade distributions and no
    way to connect it to the line they edited. That is the single most likely
    way a competent author hits a wall this pipeline could have explained, so
    the failure names the authored scalars actually in play and which way to
    move them.

    Best-effort: returns "" when the report or spec cannot be read, so a
    diagnosis failure never masks the underlying build failure.
    """
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
        zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return ""

    accessible = report.get("accessible") or {}
    steep_fraction = accessible.get("steep_edge_fraction")
    steep_threshold = accessible.get("steep_grade_threshold")
    maximum_grade = accessible.get("maximum_grade")

    lines: list[str] = []
    if isinstance(steep_fraction, (int, float)) and steep_fraction > 0.01:
        lines.append(
            "  Too much walkable ground is steep: %.3f%% of it exceeds grade %s "
            "(limit 1.000%%)." % (steep_fraction * 100.0, steep_threshold)
        )
    if isinstance(maximum_grade, (int, float)) and maximum_grade > 12.0:
        lines.append(
            "  A walkable slope reaches grade %.2f (limit 12.00)." % maximum_grade
        )
    if not lines:
        return ""

    # The jitters roughen the ridge flanks and are what usually pushes
    # accessible ground past the steepness limit; spine_count and
    # elevation_bias change mass rather than local roughness.
    steepening: dict[str, list[str]] = {}
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict) or feature.get("category") != "landform":
            continue
        composition = feature.get("generation", {}).get("composition", {})
        if not isinstance(composition, dict):
            continue
        for knob in ("along_jitter", "cross_jitter"):
            value = composition.get(knob)
            if isinstance(value, (int, float)) and value > 0.25:
                steepening.setdefault(knob, []).append(
                    "%s=%g" % (feature.get("id"), value)
                )

    lines.insert(0, "Terrain accessibility gate failed.")
    if steepening:
        for knob, entries in sorted(steepening.items()):
            lines.append(
                "  %s is high on: %s -- lower these first; the jitters roughen "
                "ridge flanks and are the usual cause." % (knob, ", ".join(entries))
            )
        lines.append(
            "  Note: worldbuilder documents the jitters as 0.0-0.5, but the "
            "buildable range is narrower and world-dependent. A legal value "
            "can still fail this gate."
        )
    else:
        lines.append(
            "  No authored jitter is unusually high, so the steepness is "
            "coming from landform elevation range or profile rather than "
            "composition. Check generation.elevation_m."
        )
    return "\n".join(lines)


def _hydrology_diagnosis(report_path: Path, zone_spec_path: Path) -> str:
    """Translate a failed hydrology uphill-step gate into an authoring repair.

    The gate reports per-stream uphill-step fraction and maximum step height,
    which name neither the authored knob at fault nor the fix. `channel_profile`
    is the authored property that determines whether a bed is forced downhill
    (`incised_stream`), left to follow local relief (`surface_channel`), or
    exempt from the flow requirement entirely (`wetland_rill`). A stream
    authored as `surface_channel` through terrain with real elevation gain is
    the single most likely cause, so the failure names it directly.

    Best-effort: returns "" when the report or spec cannot be read, so a
    diagnosis failure never masks the underlying build failure.
    """
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
        zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return ""

    policy = report.get("policy") or {}
    maximum_fraction = policy.get("maximum_hydrology_uphill_fraction")
    maximum_step_m = policy.get("maximum_hydrology_uphill_step_m")

    failing: list[dict[str, Any]] = []
    for stream in report.get("hydrology") or []:
        if not isinstance(stream, dict) or stream.get("channel_profile") == "wetland_rill":
            continue  # exempt from the downhill-continuity requirement
        fraction = stream.get("uphill_step_fraction")
        step_m = stream.get("maximum_uphill_step_m")
        exceeds_fraction = (
            isinstance(fraction, (int, float))
            and isinstance(maximum_fraction, (int, float))
            and fraction > maximum_fraction
        )
        exceeds_step = (
            isinstance(step_m, (int, float))
            and isinstance(maximum_step_m, (int, float))
            and step_m > maximum_step_m
        )
        if exceeds_fraction or exceeds_step:
            failing.append(stream)

    if not failing:
        return ""

    authored_by_id = {
        str(feature.get("id")): feature
        for feature in zone_spec.get("features", [])
        if isinstance(feature, dict) and feature.get("category") == "hydrology"
    }

    lines = ["Hydrology uphill-step gate failed."]
    for stream in failing:
        stream_id = str(stream.get("id"))
        profile = stream.get("channel_profile")
        fraction = stream.get("uphill_step_fraction")
        step_m = stream.get("maximum_uphill_step_m")
        lines.append(
            "  %s (channel_profile=%s): uphill-step fraction %.4f (limit %s), "
            "maximum uphill step %.4f m (limit %s m)."
            % (stream_id, profile, fraction or 0.0, maximum_fraction, step_m or 0.0, maximum_step_m)
        )
        if profile == "surface_channel":
            lines.append(
                "    Authored as surface_channel, which follows local relief "
                "instead of forcing a downhill bed. Set properties.channel_profile "
                "to \"incised_stream\" on feature %s to carve a monotonic bed, or "
                "to \"wetland_rill\" if this is meant to pool rather than flow."
                % stream_id
            )
        elif profile == "incised_stream":
            feature = authored_by_id.get(stream_id, {})
            point_count = len(feature.get("geometry", {}).get("points", []))
            lines.append(
                "    Already incised_stream, so the bed should be forced "
                "downhill by construction. This usually means the authored "
                "polyline (%d points) has a sharp reversal or near-flat "
                "endpoints that make the downstream direction ambiguous -- "
                "add intermediate waypoints along feature %s so the endpoint "
                "heights unambiguously order the flow direction, then rebuild."
                % (point_count, stream_id)
            )
        else:
            lines.append(
                "    channel_profile=%r on feature %s is not one of the "
                "profiles this gate exempts or forces downhill; check it "
                "against worldbuilder_dsl's documented values." % (profile, stream_id)
            )
    return "\n".join(lines)


_TRAVERSAL_FAILURE_PATTERNS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"^Lane (?P<lane>.+) is narrower than the traversal agent contract$"), "lane_width"),
    (re.compile(r"^Lane (?P<lane>.+) exceeds maximum longitudinal grade$"), "lane_long_grade"),
    (re.compile(r"^Lane (?P<lane>.+) exceeds p95 longitudinal grade$"), "lane_p95_grade"),
    (re.compile(r"^Lane (?P<lane>.+) exceeds maximum cross grade$"), "lane_cross_grade"),
    (re.compile(r"^Lane (?P<lane>.+) does not connect both faction keep approaches$"), "lane_keep_distance"),
    (re.compile(r"^Lane (?P<lane>.+) endpoints do not connect opposing keeps$"), "lane_opposing_keeps"),
    (re.compile(r"^Lane (?P<lane>.+) crosses (?P<stream>.+) without an authored traversal link$"), "lane_stream_crossing"),
    (re.compile(r"^Keep (?P<keep>.+) has no safe lane approach$"), "keep_approach"),
    (re.compile(r"^Keep (?P<keep>.+) spawn anchor exceeds local slope limit$"), "keep_slope"),
    (re.compile(r"^Objective (?P<objective>.+) is disconnected from the lane network$"), "objective_approach"),
    (re.compile(r"^Objective (?P<objective>.+) anchor exceeds local slope limit$"), "objective_slope"),
    (re.compile(r"^Blocking lane (?P<lane>.+) leaves no opposing-keep replan route$"), "blocking_lane"),
]


def _traversal_diagnosis(traversal_report: dict[str, Any], zone_spec: dict[str, Any]) -> str:
    """Translate failed traversal probes into the authored input and repair.

    The probe reports which lane/keep/objective failed and by what rule, but
    not which authored value to change. Every failure category below maps to
    a specific zone_spec field (lane `properties.minimum_width_m`, route
    `geometry.points`, `traversal_policy.*`, or a missing bridge feature), so
    the message names that field and the measured value already computed by
    the probe rather than sending the author back to raw grade percentiles.

    Best-effort: unrecognised failure strings are passed through verbatim
    rather than dropped, so a probe change that adds a new failure category
    degrades to "unexplained" instead of silently losing the failure.
    """
    failures = traversal_report.get("failures") or []
    if not failures:
        return ""

    policy = traversal_report.get("policy") or {}
    lanes = traversal_report.get("lanes") or {}
    keeps = traversal_report.get("keeps") or {}
    objectives = traversal_report.get("objectives") or {}
    features_by_id = {
        str(feature.get("id")): feature
        for feature in zone_spec.get("features", [])
        if isinstance(feature, dict)
    }

    def lane_feature(lane_id: str) -> dict[str, Any]:
        feature_id = lanes.get(lane_id, {}).get("feature_id")
        return features_by_id.get(str(feature_id), {})

    lines = ["Traversal probe failed."]
    unrecognised: list[str] = []
    for failure in failures:
        matched = False
        for pattern, kind in _TRAVERSAL_FAILURE_PATTERNS:
            match = pattern.match(failure)
            if not match:
                continue
            matched = True
            group = match.groupdict()
            if kind == "lane_width":
                lane_id = group["lane"]
                report = lanes.get(lane_id, {})
                required = max(
                    (
                        clearance["minimum_required_width_m"]
                        for clearance in report.get("actor_clearance", {}).values()
                    ),
                    default=None,
                )
                lines.append(
                    "  Lane %s: authored_width_m=%s is narrower than the largest "
                    "actor's minimum_required_width_m=%s. Raise "
                    "properties.minimum_width_m on the lane feature, or lower "
                    "traversal_policy.agent_radius_m."
                    % (lane_id, report.get("authored_width_m"), required)
                )
            elif kind in ("lane_long_grade", "lane_p95_grade", "lane_cross_grade"):
                lane_id = group["lane"]
                report = lanes.get(lane_id, {})
                metric, limit_key = {
                    "lane_long_grade": ("maximum_longitudinal_grade", "maximum_lane_grade"),
                    "lane_p95_grade": ("p95_longitudinal_grade", "maximum_lane_p95_grade"),
                    "lane_cross_grade": ("maximum_cross_grade", "maximum_lane_cross_grade"),
                }[kind]
                lines.append(
                    "  Lane %s: %s=%s exceeds traversal_policy.%s=%s. The route's "
                    "geometry.points crosses terrain steeper than this policy "
                    "allows -- move the waypoints away from the steep ground, or "
                    "check nearby landform generation.composition (along_jitter / "
                    "cross_jitter / elevation_m) for the terrain the route "
                    "crosses."
                    % (lane_id, metric, report.get(metric), limit_key, policy.get(limit_key))
                )
            elif kind == "lane_keep_distance":
                lane_id = group["lane"]
                report = lanes.get(lane_id, {})
                lines.append(
                    "  Lane %s: an endpoint is farther than "
                    "traversal_policy.maximum_keep_lane_distance_m=%s from its "
                    "nearest keep (endpoint_assignments=%s). Move the lane's "
                    "geometry.points endpoint closer to the keep's "
                    "geometry.points, or move the keep, or raise the policy "
                    "distance."
                    % (
                        lane_id,
                        policy.get("maximum_keep_lane_distance_m"),
                        report.get("endpoint_assignments"),
                    )
                )
            elif kind == "lane_opposing_keeps":
                lane_id = group["lane"]
                report = lanes.get(lane_id, {})
                lines.append(
                    "  Lane %s: both endpoints resolve to the same nearest keep "
                    "(endpoint_assignments=%s) instead of connecting opposing "
                    "keeps. Re-route geometry.points so each endpoint sits near "
                    "a different faction_keep landmark."
                    % (lane_id, report.get("endpoint_assignments"))
                )
            elif kind == "lane_stream_crossing":
                lane_id, stream_id = group["lane"], group["stream"]
                lines.append(
                    "  Lane %s crosses hydrology feature %s with no bridge "
                    "authored. Add a structure feature with semantic=\"bridge\" "
                    "and properties.derived_from=[\"%s\", \"%s\"], or re-route "
                    "the lane's geometry.points to avoid the stream."
                    % (lane_id, stream_id, lane_id, stream_id)
                )
            elif kind == "keep_approach":
                keep_id = group["keep"]
                report = keeps.get(keep_id, {})
                lines.append(
                    "  Keep %s: nearest_lane_distance_m=%s exceeds "
                    "traversal_policy.maximum_keep_lane_distance_m=%s. Move the "
                    "keep's geometry.points closer to a lane, or extend a lane "
                    "toward it."
                    % (
                        keep_id,
                        report.get("nearest_lane_distance_m"),
                        policy.get("maximum_keep_lane_distance_m"),
                    )
                )
            elif kind == "objective_approach":
                objective_id = group["objective"]
                report = objectives.get(objective_id, {})
                lines.append(
                    "  Objective %s: nearest_lane_distance_m=%s exceeds "
                    "traversal_policy.maximum_objective_lane_distance_m=%s. Move "
                    "the objective's geometry.points closer to a lane, or extend "
                    "a lane toward it."
                    % (
                        objective_id,
                        report.get("nearest_lane_distance_m"),
                        policy.get("maximum_objective_lane_distance_m"),
                    )
                )
            elif kind in ("keep_slope", "objective_slope"):
                entity_id = group.get("keep") or group.get("objective")
                report_map = keeps if kind == "keep_slope" else objectives
                report = report_map.get(entity_id, {})
                lines.append(
                    "  %s %s: maximum_local_grade=%s at the spawn anchor exceeds "
                    "traversal_policy.maximum_lane_cross_grade=%s. Move the "
                    "landmark's geometry.points to flatter ground, or reduce "
                    "nearby landform generation.composition jitter."
                    % (
                        "Keep" if kind == "keep_slope" else "Objective",
                        entity_id,
                        report.get("maximum_local_grade"),
                        policy.get("maximum_lane_cross_grade"),
                    )
                )
            elif kind == "blocking_lane":
                lane_id = group["lane"]
                lines.append(
                    "  Removing lane %s leaves no viable alternative route "
                    "between opposing keeps -- every other lane already fails a "
                    "gate above, or too few lanes are authored. Fix the other "
                    "lanes' failures first, or author an additional lane "
                    "connecting the same two keeps by a different route."
                    % lane_id
                )
            break
        if not matched:
            unrecognised.append(failure)
    if unrecognised:
        lines.append("  Unexplained (probe added a failure this diagnosis does not recognise yet):")
        for failure in unrecognised:
            lines.append("    %s" % failure)
    return "\n".join(lines)


_CONTRACT_TRIAGE: list[tuple[re.Pattern[str], str]] = [
    (
        re.compile(r"does not match the ZoneSpec$"),
        "The batch's zone_id disagrees with the current zone_spec.json. This is "
        "not an authoring mistake -- it means the terrain/analysis on disk was "
        "built from a different ZoneSpec than the one being validated now "
        "(stale batch directory, or the wrong batch was passed). Rebuild the "
        "batch from scratch rather than editing the ZoneSpec.",
    ),
    (
        re.compile(r"does not match the supplied artifact$"),
        "A stored sha256 does not match the bytes of the artifact it claims to "
        "hash. This is not an authoring mistake -- some stage wrote a report or "
        "manifest before (or without) regenerating the file it hashes, so the "
        "two are out of sync. Delete the batch's terrain/ directory and rebuild "
        "from scratch; do not hand-edit any .bin or manifest file.",
    ),
    (
        re.compile(r"has \d+ bytes; expected \d+$"),
        "A binary artifact (heightfield or mask) is the wrong size for the "
        "manifest's declared resolution. This is not an authoring mistake -- "
        "the terrain raster and the manifest describing it were generated at "
        "different resolutions. Rebuild the batch from scratch.",
    ),
    (
        re.compile(r"resolution does not match the terrain manifest$"),
        "This is not an authoring mistake -- the terrain analysis report and "
        "the terrain manifest disagree on resolution, meaning two different "
        "builds got mixed together. Rebuild the batch from scratch rather "
        "than reusing any individual artifact.",
    ),
    (
        re.compile(r"schema_version must be"),
        "The analyzer's output schema_version does not match what this "
        "validator expects. This is a pipeline version mismatch (analyzer and "
        "validator built from different commits), not an authoring issue -- "
        "make sure terrain_lab and world_core are built from the same "
        "checkout, then rebuild.",
    ),
]


def _terrain_contract_diagnosis(message: str) -> str:
    """Triage a failed terrain-contract validation (Rust `validate-terrain-analysis`).

    Unlike the accessibility, hydrology, and traversal gates, this one checks
    provenance and internal consistency between pipeline stages (hashes,
    byte sizes, resolutions, schema versions) -- it is not a policy an author
    tunes through the DSL. Most failures here mean a stale or out-of-order
    artifact, not a bad authored value, so the honest "repair" is almost
    always "rebuild the batch from scratch" rather than a DSL edit. Naming
    that plainly is the point: a diagnosis that invents a fake authored knob
    for a provenance bug would send an author chasing the wrong fix.

    Best-effort: unrecognised messages get a generic pipeline-bug note rather
    than nothing, so a new Contract error still triages as "not authoring."
    """
    for pattern, triage in _CONTRACT_TRIAGE:
        if pattern.search(message):
            return "Terrain contract validation failed.\n  %s" % triage
    return (
        "Terrain contract validation failed.\n"
        "  This is an internal pipeline consistency check, not an authoring "
        "gate -- it did not match a known failure category. It most likely "
        "indicates a bug in the terrain analyzer or validator rather than an "
        "authored value to change. Rebuilding the batch from scratch will "
        "clear a stale-artifact cause; if it recurs on a clean rebuild, report "
        "the exact message upstream."
    )


def _analyze_and_validate_terrain(
    project_root: Path, batch_dir: Path, zone_spec_path: Path
) -> dict[str, Any]:
    """Run Julia numerics, then let Rust certify provenance and policy."""
    codeweald_root = _engine_root()
    terrain_lab = codeweald_root / "terrain_lab"
    world_core = codeweald_root / "world_core"
    terrain_dir = batch_dir / "terrain"
    report_path = terrain_dir / "terrain_analysis.json"
    julia = shutil.which("julia") or "julia"
    cargo = shutil.which("cargo") or "cargo"
    try:
        _run(
            [
                julia,
                "--project=" + str(terrain_lab),
                "--startup-file=no",
                str(terrain_lab / "bin/analyze_heightfield.jl"),
                "--heightfield",
                str(terrain_dir / "heightfield_f32le.bin"),
                "--protected-mask",
                str(terrain_dir / "protected_relief_mask.bin"),
                "--semantic-region-mask",
                str(terrain_dir / "semantic_region_mask.bin"),
                "--manifest",
                str(terrain_dir / "terrain_manifest.json"),
                "--output",
                str(report_path),
            ],
            codeweald_root,
        )
    except ZoneCompileError as exc:
        # The analyzer writes its report before exiting non-zero, so the
        # numbers behind the failure are on disk and can be turned into an
        # authoring repair instead of a slope percentile.
        diagnoses = [
            text
            for text in (
                _accessibility_diagnosis(report_path, zone_spec_path),
                _hydrology_diagnosis(report_path, zone_spec_path),
            )
            if text
        ]
        if diagnoses:
            raise ZoneCompileError(
                "%s\n%s" % (exc.args[0], "\n".join(diagnoses))
            ) from exc
        raise
    try:
        _run(
            [
                cargo,
                "run",
                "--quiet",
                "--manifest-path",
                str(world_core / "Cargo.toml"),
                "--bin",
                "codeweald-worldspec",
                "--",
                "validate-terrain-analysis",
                str(zone_spec_path),
                str(terrain_dir / "terrain_manifest.json"),
                str(terrain_dir / "heightfield_f32le.bin"),
                str(terrain_dir / "protected_relief_mask.bin"),
                str(terrain_dir / "semantic_region_mask.bin"),
                str(report_path),
            ],
            codeweald_root,
        )
    except ZoneCompileError as exc:
        raise ZoneCompileError(
            "%s\n%s" % (exc.args[0], _terrain_contract_diagnosis(exc.args[0]))
        ) from exc
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report.get("status") != "passed":
        # Defensive: the Julia analyzer already exits non-zero on a failed
        # report (caught above with the accessibility/hydrology diagnosis),
        # so a "failed" status reaching here means the contract validator
        # accepted a report the analyzer itself marked failed -- a pipeline
        # inconsistency, not something this branch should ever see live.
        raise ZoneCompileError(
            "Terrain analysis did not pass its declared policy\n%s"
            % _terrain_contract_diagnosis("")
        )
    return report


def _solve_and_validate_placements(
    project_root: Path,
    batch_dir: Path,
    zone_spec: dict[str, Any],
    asset_plan: dict[str, Any],
) -> dict[str, Any]:
    """Run Julia's spatial solver and certify its concrete transforms in Rust."""
    codeweald_root = _engine_root()
    terrain_lab = codeweald_root / "terrain_lab"
    world_core = codeweald_root / "world_core"
    terrain_dir = batch_dir / "terrain"
    placement_path = batch_dir / "placement_plan.json"
    julia = shutil.which("julia") or "julia"
    cargo = shutil.which("cargo") or "cargo"
    _run(
        [
            julia,
            "--project=" + str(terrain_lab),
            "--startup-file=no",
            str(terrain_lab / "bin/solve_landform_placements.jl"),
            "--zone-spec",
            str(batch_dir / "zone_spec.json"),
            "--asset-plan",
            str(batch_dir / "asset_plan.json"),
            "--heightfield",
            str(terrain_dir / "heightfield_f32le.bin"),
            "--manifest",
            str(terrain_dir / "terrain_manifest.json"),
            "--zone-spec-sha256",
            _canonical_sha256(zone_spec),
            "--asset-plan-sha256",
            _canonical_sha256(asset_plan),
            "--output",
            str(placement_path),
        ],
        codeweald_root,
    )
    _run(
        [
            cargo,
            "run",
            "--quiet",
            "--manifest-path",
            str(world_core / "Cargo.toml"),
            "--bin",
            "codeweald-worldspec",
            "--",
            "validate-plan",
            str(batch_dir / "zone_spec.json"),
            str(batch_dir / "asset_plan.json"),
            str(placement_path),
        ],
        codeweald_root,
    )
    return json.loads(placement_path.read_text(encoding="utf-8"))


def _compile_render_plan(
    project_root: Path,
    batch_dir: Path,
) -> dict[str, Any]:
    """Freeze every renderer-visible asset transform behind Rust validation."""
    codeweald_root = _engine_root()
    world_core = codeweald_root / "world_core"
    terrain_dir = batch_dir / "terrain"
    render_plan_path = batch_dir / "render_plan.json"
    cargo = shutil.which("cargo") or "cargo"
    _run(
        [
            cargo,
            "run",
            "--quiet",
            "--manifest-path",
            str(world_core / "Cargo.toml"),
            "--bin",
            "codeweald-worldspec",
            "--",
            "compile-render-plan",
            str(batch_dir / "zone_spec.json"),
            str(batch_dir / "asset_plan.json"),
            str(
                batch_dir
                / "asset_visual_preflight"
                / "asset_visual_preflight_report.json"
            ),
            str(terrain_dir / "terrain_manifest.json"),
            str(terrain_dir / "heightfield_f32le.bin"),
            str(batch_dir / "placement_plan.json"),
            str(render_plan_path),
        ],
        codeweald_root,
    )
    return json.loads(render_plan_path.read_text(encoding="utf-8"))


def apply_style_policy(
    visual: dict[str, Any], style_report: dict[str, Any], policy: dict[str, Any]
) -> dict[str, Any]:
    """Attach style evidence and enforce the reviewed batch's failure policy."""
    threshold = float(policy.get("minimum_style_score", 0.42))
    mismatch = str(policy.get("style_mismatch", "fail"))
    style_value = float(style_report.get("score", 0.0))
    visual["style_match"] = style_report
    visual["style_review_threshold"] = threshold
    visual["style_mismatch_policy"] = mismatch
    visual["fidelity_status"] = "reviewed_style_range" if style_value >= threshold else "needs_art_direction"
    if style_value < threshold:
        message = "Rendered style score %.3f is below the %.2f reviewed fidelity threshold" % (style_value, threshold)
        target = "failures" if mismatch == "fail" else "warnings"
        visual.setdefault(target, []).append(message)
        if mismatch == "fail":
            visual["status"] = "failed"
        elif visual.get("status") == "passed":
            visual["status"] = "warnings"
    return visual


def _canonical_source_path(zone_spec: dict[str, Any], batch_dir: Path) -> Path:
    zone = zone_spec.get("zone", {})
    sources = zone.get("source_images", [])
    reconciliation = zone.get("image_reconciliation", {})
    canonical_id = reconciliation.get("canonical_map_image_id")
    source = next(
        (
            item
            for item in sources
            if isinstance(item, dict) and item.get("id") == canonical_id
        ),
        None,
    )
    if not isinstance(source, dict) or not isinstance(source.get("path"), str):
        raise ZoneCompileError("ZoneSpec has no canonical source image")
    return batch_dir / str(source["path"])


def _provenance_path(path: Path, batch_dir: Path, project_root: Path) -> str:
    """How the spec and the build report name an input or artifact.

    **Batch-relative first, and that ordering is load-bearing.** These strings
    are hashed into `zone_spec_canonical_sha256` -- the world's identity. Naming
    a batch file by an engine-relative or absolute path folds the batch's
    location on disk into that identity, so the same world compiled in two
    places is two different worlds (D22). Batch-relative is the only name that
    describes the world rather than the machine it was built on.

    Engine-relative is the fallback for genuine engine assets; absolute is the
    last resort, and reaching it means the file belongs to neither, which is
    worth being visible in the report rather than silently rewritten.
    """
    for root in (batch_dir, project_root):
        try:
            return path.relative_to(root).as_posix()
        except ValueError:
            continue
    return path.as_posix()


def build(
    annotations: Path, project_root: Path, asset_root: Path, *, run_godot: bool = False,
    capture: bool = False, godot_bin: str = "godot", blender_bin: str = "blender",
    cross_engine_handoffs: bool = False, preview_active_on_failure: bool = False,
    catalog_path: Path | None = None,
) -> dict[str, Any]:
    project_root = project_root.resolve()
    # Relative to the *project*, never to the caller's working directory. The
    # `assets` default was resolved against cwd, which was invisibly correct
    # only while every build was run from inside the project. Once the engine
    # lives somewhere else (D8), cwd is the engine and `assets` points at
    # nothing -- as a missing material manifest, three stages downstream.
    asset_root = asset_root if asset_root.is_absolute() else (project_root / asset_root)
    asset_root = asset_root.resolve()
    annotations = annotations.resolve()
    # A batch does not have to live inside the engine. It used to, because the
    # Godot subprocess needs a `res://`-relative batch path and the guard was
    # written for it -- but that requirement belongs to Godot, not to WGE, and
    # applying it to every build is what stopped the compiler from being
    # pointable at a scratch directory (D8). A headless build now reads its
    # inputs from wherever the batch is and writes every artifact beside it.
    in_tree = project_root in annotations.parents
    if run_godot and not in_tree:
        raise ZoneCompileError(
            "Concept annotations must be inside project root to compile a Godot "
            "scene, which addresses batches as res:// paths. A headless build "
            "(no --godot/--capture) has no such restriction."
        )
    batch_dir = annotations.parent
    build_report_path = batch_dir / "build_report.json"
    build_marker_path = batch_dir / ".build-in-progress"
    # A prior green report must never survive beside a failed new revision.
    # Later stages write a fresh running/failed/passed report as soon as enough
    # provenance exists to identify the attempted build.
    build_report_path.unlink(missing_ok=True)
    build_marker_path.write_text(
        json.dumps(
            {
                "schema_version": "codeweald.build-transaction/v1",
                "annotations": annotations.name,
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    # Only meaningful to the Godot compiler, which is the only consumer; an
    # out-of-tree batch has no res:// address and never reaches that path.
    batch_relative = batch_dir.relative_to(project_root).as_posix() if in_tree else None
    terrain_dir = batch_dir / "terrain"
    result = compile_file(annotations, batch_dir / "zone_spec.json", batch_dir / "validation_report.json")
    world_intent_source = batch_dir / "world_intent.py"
    world_intent: dict[str, Any] | None = None
    if world_intent_source.is_file():
        world_intent = compile_intent(
            world_intent_source.read_text(encoding="utf-8"),
            source_name=_provenance_path(world_intent_source, batch_dir, project_root),
        )
        patched_zone_spec = apply_intent(result.zone_spec, world_intent)
        result.zone_spec.clear()
        result.zone_spec.update(patched_zone_spec)
        result.report["world_intent"] = {
            "source": _provenance_path(world_intent_source, batch_dir, project_root),
            "patch_count": len(world_intent["patches"]),
            "execution_policy": "parsed_not_executed",
        }
        _write(batch_dir / "world_intent.json", world_intent)
        _write(batch_dir / "zone_spec.json", result.zone_spec)
        _write(batch_dir / "validation_report.json", result.report)
    evidence_overlay = render_overlay(result.zone_spec, batch_dir, batch_dir / "evidence_overlay.png")
    _write(batch_dir / "evidence_report.json", evidence_overlay)
    source_path = _canonical_source_path(result.zone_spec, batch_dir)
    reference_style = style_profile(source_path)
    _write(batch_dir / "style_reference.json", reference_style)
    baseline_style_calibration = initial_style_calibration(reference_style)
    _write(batch_dir / "style_calibration.json", baseline_style_calibration)
    (batch_dir / "style_calibration_report.json").unlink(missing_ok=True)
    runtime_effects = build_runtime_effects(result.zone_spec)
    _write(batch_dir / "runtime_effects.json", runtime_effects)
    terrain = rasterize_zone_spec(result.zone_spec, source_root=batch_dir)
    write_raster(terrain, terrain_dir)
    terrain_analysis = _analyze_and_validate_terrain(
        project_root, batch_dir, batch_dir / "zone_spec.json"
    )
    traversal = evaluate_traversal(result.zone_spec, terrain.height_m)
    _write(terrain_dir / "traversal_probe_report.json", traversal)
    if traversal["status"] != "passed":
        diagnosis = _traversal_diagnosis(traversal, result.zone_spec)
        message = "Compiled terrain failed traversal probes"
        if diagnosis:
            message = "%s\n%s" % (message, diagnosis)
        raise ZoneCompileError(message)
    terrain.manifest["terrain_materials"] = resolve_terrain_materials(
        result.zone_spec.get("terrain_materials", {}),
        project_root,
        asset_root,
        result.zone_spec.get("terrain_material_scale_m", {}),
    )
    terrain.manifest["baked_material_preview"] = bake_material_preview(
        project_root=project_root,
        output_dir=terrain_dir,
        splat_rgba=terrain.splat_rgba,
        wetland_mask=terrain.wetland_mask,
        materials=terrain.manifest["terrain_materials"],
        width_m=float(terrain.manifest["world_bounds_m"]["width"]),
        length_m=float(terrain.manifest["world_bounds_m"]["length"]),
    )
    _write(terrain_dir / "terrain_manifest.json", terrain.manifest)
    godot_executable: str | None = None
    if run_godot:
        # Import readiness is catalog evidence, so Godot must scan newly
        # generated assets before the catalog and asset plan are frozen.
        # Importing only immediately before scene compilation leaves a stale
        # plan that still calls valid new GLBs unavailable.
        godot_executable = shutil.which(godot_bin) or godot_bin
        _run(
            [godot_executable, "--headless", "--path", str(project_root), "--import"],
            project_root,
        )
    catalog = build_catalog(project_root, asset_root)
    # Defaults into the engine tree, which is right for a normal build and
    # wrong for a sandbox: a shared read-only WGE cannot be written to, and two
    # concurrent runs would race over one file. An explicit path lets a
    # throwaway batch keep its catalog beside itself.
    catalog_path = catalog_path or (project_root / "assets/generated/codeweald_asset_catalog.json")
    _write(catalog_path, catalog)
    asset_plan = resolve_asset_plan(result.zone_spec, catalog)
    _write(batch_dir / "asset_plan.json", asset_plan)
    asset_preflight = preflight_assets(batch_dir / "asset_plan.json", project_root, batch_dir / "asset_visual_preflight", blender_bin=blender_bin)
    # Hard gate, and the right place for the affordance contract to land: an
    # asset that cannot do its role's job should never reach placement, let
    # alone be discovered three stages later by `navigation_plan` (D17).
    asset_physical = evaluate_asset_physical(
        asset_plan,
        asset_preflight,
        traversal_policy=result.zone_spec.get("traversal_policy"),
        asset_root=project_root,
    )
    _write(batch_dir / "asset_physical_acceptance_report.json", asset_physical)
    if asset_physical["status"] != "passed":
        raise ZoneCompileError("Selected assets failed physical-bounds acceptance")
    placement_plan = _solve_and_validate_placements(
        project_root, batch_dir, result.zone_spec, asset_plan
    )
    render_plan = _compile_render_plan(project_root, batch_dir)
    # Derived from the render plan and the certified heightfield, so it runs
    # after both and is hash-bound to each. Without this the compiler emits a
    # world that renders and nothing can move through.
    collision = build_collision_plan(batch_dir, project_root)
    _write(batch_dir / "collision_plan.json", collision)
    # Roadmap 1.5: no walkable surface without a collider, no collider
    # floating off (or sunk into) terrain. Soft gate, same reasoning as
    # boundary_plan/navmesh_acceptance -- reports and warns, does not fail
    # the build, because a placement defect found here is not this stage's
    # to fix.
    collision_acceptance = evaluate_collision(
        collision,
        render_plan,
        terrain.height_m,
        float(terrain.manifest["world_bounds_m"]["width"]),
        float(terrain.manifest["world_bounds_m"]["length"]),
    )
    _write(batch_dir / "collision_acceptance.json", collision_acceptance)
    if collision_acceptance["status"] != "passed":
        print(
            "  collision: FAILED -- %d requirement(s) not met (see "
            "collision_acceptance.json, roadmap 1.5)"
            % len(collision_acceptance["failures"])
        )
    # Measures where the spec's own agent can actually get, and reports the
    # spans of world edge it can walk off. Deliberately does not fail the build
    # yet: the leak is real and open (roadmap 2.5 closes it with landform), and
    # a build that refuses to produce the artifact naming the defect is worse
    # than one that produces it loudly.
    boundary = write_boundary_plan(batch_dir)
    _write(batch_dir / "boundary_plan.json", boundary)
    if not boundary["containment"]["enclosed"]:
        print(
            "  boundary: NOT ENCLOSED -- %.0f m of world edge across %d spans is "
            "walkable off into void (see boundary_plan.json, roadmap 2.5)"
            % (
                boundary["containment"]["leak_length_m"],
                len(boundary["containment"]["leak_spans"]),
            )
        )
    # Reads the collision plan, so it runs after it. This is where the world
    # stops being terrain-only: `boundary_plan` measures slope alone and counts
    # ground inside solid objects as playable.
    # The only measured target for the composition knobs. Everything that
    # sounded like it measured landform shape turned out to be a texture metric
    # (tooling items 3 and 6), so without this the geometry knobs optimise
    # nothing. Emitted, not gated: it describes the world rather than judging it.
    silhouette = measure_silhouette(batch_dir)
    _write(batch_dir / "silhouette.json", silhouette)
    print(
        "  silhouette: relief %.2f deg, %.1f peaks per turn, coherence %.3f"
        % (
            silhouette["metrics"]["horizon_relief_deg"],
            silhouette["metrics"]["horizon_peaks_per_turn"],
            silhouette["metrics"]["horizon_coherence"],
        )
    )
    navigation = write_navigation_plan(batch_dir)
    _write(batch_dir / "navigation_plan.json", navigation)
    for lane in navigation["lanes"]:
        if lane["runs_end_to_end"]:
            continue
        blocking = ", ".join(entry["id"] for entry in lane["obstructions"][:3]) or "terrain"
        print(
            "  navigation: lane %s does not run end to end (longest gap %.0f m, "
            "blocked by %s)"
            % (lane["id"], lane["longest_impassable_run_m"], blocking)
        )
    if navigation["topology"]["stranded_anchors"]:
        print(
            "  navigation: not connected to the keeps: %s"
            % ", ".join(navigation["topology"]["stranded_anchors"])
        )
    # Roadmap 1.4: the verdict navigation_plan's evidence is for. Deliberately
    # does not fail the build -- lanes not running end to end is D13
    # (settlements placed on lane centrelines), a known-open placement defect
    # with a named owner, not a build-time regression, and a build that
    # refuses to produce the artifact naming the defect is worse than one
    # that produces it loudly (same reasoning as boundary_plan). Once D13 is
    # fixed, wire `navmesh_acceptance.py --require-passed` in here as a real
    # exit-path gate -- a soft gate with no task named to harden it just sits
    # soft forever.
    navmesh = evaluate_navmesh(navigation)
    _write(batch_dir / "navmesh_acceptance.json", navmesh)
    if navmesh["status"] != "passed":
        print(
            "  navmesh: FAILED -- %d requirement(s) not met (see "
            "navmesh_acceptance.json, roadmap 1.4)" % len(navmesh["failures"])
        )
    _write(batch_dir / "godot_asset_plan.json", adapt_asset_plan(asset_plan))
    stages = [
        "compile",
        "evidence_overlay",
        "style_reference",
        "runtime_effects",
        "terrain",
        "terrain_analysis",
        "terrain_contract",
        "traversal_probe",
        "terrain_materials",
    ]
    if world_intent is not None:
        stages.insert(1, "worldbuilder_dsl")
    if run_godot:
        stages.append("godot_import")
    stages.extend(
        [
            "asset_catalog",
            "asset_plan",
            "asset_visual_preflight",
            "asset_physical_acceptance",
            "julia_placement_solver",
            "rust_placement_contract",
            "rust_render_plan",
            "collision_plan",
            "collision_acceptance",
            "boundary_plan",
            "silhouette",
            "navigation_plan",
            "navmesh_acceptance",
            "godot_adapter",
        ]
    )
    if cross_engine_handoffs:
        # Dormant compatibility path. Godot is the sole production target;
        # these handoffs are emitted only when explicitly requested.
        from unreal_artifacts import write_unreal_artifacts
        from zone_to_unity import adapt_unity
        from zone_to_unreal import adapt_unreal
        terrain.manifest.setdefault("engine_artifacts", {})["unreal"] = write_unreal_artifacts(terrain_dir)
        _write(terrain_dir / "terrain_manifest.json", terrain.manifest)
        _write(batch_dir / "unity_zone_import.json", adapt_unity(result.zone_spec, terrain.manifest, asset_plan, runtime_effects, project_root))
        _write(batch_dir / "unreal_zone_import.json", adapt_unreal(result.zone_spec, terrain.manifest, asset_plan, runtime_effects))
        stages.extend(["unity_adapter", "unreal_adapter"])

    reports: dict[str, Any] = {
        "schema_version": BUILD_VERSION,
        "status": "running",
        "zone_id": result.zone_spec["zone"]["id"],
        "input_annotations": {
            "path": _provenance_path(annotations, batch_dir, project_root),
            "sha256": _sha256(annotations),
        },
        "zone_spec_sha256": _sha256(batch_dir / "zone_spec.json"),
        "terrain_analysis": {
            "path": _provenance_path(terrain_dir / "terrain_analysis.json", batch_dir, project_root),
            "heightfield_sha256": terrain_analysis["heightfield_sha256"],
            "status": terrain_analysis["status"],
        },
        "placement_plan": {
            "path": _provenance_path(batch_dir / "placement_plan.json", batch_dir, project_root),
            "placement_count": len(placement_plan.get("placements", [])),
            "solver": placement_plan.get("solver", {}),
        },
        "render_plan": {
            "path": _provenance_path(batch_dir / "render_plan.json", batch_dir, project_root),
            "instance_count": len(render_plan.get("instances", [])),
            "counts_by_role": render_plan.get("counts_by_role", {}),
            "generator": render_plan.get("generator", {}),
        },
        "stages": stages,
    }
    _write(build_report_path, reports)
    if run_godot:
        executable = godot_executable or shutil.which(godot_bin) or godot_bin
        candidate_scene_resource = "res://.codeweald_candidate_moba_3d.tscn"
        candidate_scene_path = project_root / ".codeweald_candidate_moba_3d.tscn"
        active_scene_path = project_root / "moba_3d.tscn"
        _run(
            [
                executable, "--headless", "--path", str(project_root),
                "--script", "res://pipeline/build_zone_spec_scene.gd", "--",
                "--batch", batch_relative, "--output", candidate_scene_resource,
            ],
            project_root,
        )
        godot_build_report_path = terrain_dir / "godot_build_report.json"
        build_report = json.loads(godot_build_report_path.read_text(encoding="utf-8"))
        if build_report.get("scene_path") != candidate_scene_resource or not candidate_scene_path.is_file():
            raise ZoneCompileError("Godot compiler did not write the expected candidate scene")
        build_report["promotion_status"] = "pending_acceptance"
        # The Godot compiler owns the scene data; the orchestrator owns the
        # source-batch provenance.  Recording both closes the stale-output hole
        # where a valid scene could be mistaken for another annotation revision.
        build_report["input_annotations"] = reports["input_annotations"]
        build_report["zone_spec_sha256"] = reports["zone_spec_sha256"]
        _write(godot_build_report_path, build_report)
        reports["stages"].append("godot_scene")
        if capture:
            xvfb = shutil.which("xvfb-run")
            command = [
                executable, "--path", str(project_root), "--rendering-driver", "opengl3",
                "--script", "res://pipeline/capture_active_zone.gd", "--",
                "--batch", batch_relative, "--scene", candidate_scene_resource,
            ]
            _run([xvfb, "-a", *command] if xvfb else command, project_root)
            active_capture_path = terrain_dir / "godot_active_scene.png"
            projection_path = terrain_dir / "godot_overview_projection.json"
            first_scene_bytes = candidate_scene_path.read_bytes()
            first_capture_bytes = active_capture_path.read_bytes()
            first_projection_bytes = projection_path.read_bytes()
            first_build_report = json.loads(json.dumps(build_report))
            first_rendered_image = _image(active_capture_path)
            first_style_report = style_score(reference_style, first_rendered_image)
            if float(first_style_report.get("score", 0.0)) < float(
                result.zone_spec.get("acceptance_policy", {}).get(
                    "minimum_style_score", 0.42
                )
            ):
                # One bounded feedback pass uses the actual engine capture to
                # correct global exposure, contrast, and saturation. Geometry,
                # materials, and regional fidelity remain separately gated.
                feedback_calibration = refine_style_calibration(
                    reference_style, first_style_report
                )
                _write(batch_dir / "style_calibration.json", feedback_calibration)
                _run(
                    [
                        executable, "--headless", "--path", str(project_root),
                        "--script", "res://pipeline/build_zone_spec_scene.gd", "--",
                        "--batch", batch_relative, "--output", candidate_scene_resource,
                    ],
                    project_root,
                )
                build_report = json.loads(
                    godot_build_report_path.read_text(encoding="utf-8")
                )
                if (
                    build_report.get("scene_path") != candidate_scene_resource
                    or not candidate_scene_path.is_file()
                ):
                    raise ZoneCompileError(
                        "Godot style-calibrated compiler did not write the expected candidate scene"
                    )
                build_report["promotion_status"] = "pending_acceptance"
                build_report["input_annotations"] = reports["input_annotations"]
                build_report["zone_spec_sha256"] = reports["zone_spec_sha256"]
                _write(godot_build_report_path, build_report)
                _run([xvfb, "-a", *command] if xvfb else command, project_root)
                feedback_style_report = style_score(
                    reference_style, _image(active_capture_path)
                )
                selected_pass = select_best_pass(
                    first_style_report, feedback_style_report
                )
                if selected_pass == 0:
                    # Preserve the best measured candidate and all evidence that
                    # describes it. A feedback controller must never leave a
                    # worse scene behind merely because it ran later.
                    candidate_scene_path.write_bytes(first_scene_bytes)
                    active_capture_path.write_bytes(first_capture_bytes)
                    projection_path.write_bytes(first_projection_bytes)
                    build_report = first_build_report
                    _write(godot_build_report_path, build_report)
                    _write(
                        batch_dir / "style_calibration.json",
                        baseline_style_calibration,
                    )
                _write(
                    batch_dir / "style_calibration_report.json",
                    {
                        "schema_version": "codeweald.style-calibration-report/v1",
                        "baseline_score": first_style_report["score"],
                        "feedback_score": feedback_style_report["score"],
                        "selected_pass": selected_pass,
                        "selected_score": (
                            feedback_style_report["score"]
                            if selected_pass == 1
                            else first_style_report["score"]
                        ),
                        "feedback_adjustment": feedback_calibration["adjustment"],
                        "baseline_metrics": first_style_report.get(
                            "candidate", {}
                        ).get("metrics", {}),
                        "feedback_metrics": feedback_style_report.get(
                            "candidate", {}
                        ).get("metrics", {}),
                    },
                )
                reports["stages"].append("style_calibration_feedback")
            rendered = terrain_dir / "godot_active_scene.png"
            rendered_image = _image(rendered)
            raw_overview_projection = json.loads(
                (terrain_dir / "godot_overview_projection.json").read_text(encoding="utf-8")
            )
            overview_projection = evaluate_projection(
                result.zone_spec, raw_overview_projection,
            )
            _write(terrain_dir / "overview_projection_acceptance_report.json", overview_projection)
            raw_navigation_probe = json.loads(
                (terrain_dir / "godot_navigation_probe.json").read_text(
                    encoding="utf-8"
                )
            )
            navigation = evaluate_navigation(
                result.zone_spec, raw_navigation_probe
            )
            _write(
                terrain_dir / "navigation_acceptance_report.json", navigation
            )
            visual = evaluate_visual(
                result.zone_spec,
                _image(source_path),
                rendered_image,
                raw_overview_projection,
            )
            visual = apply_style_policy(
                visual,
                style_score(reference_style, rendered_image),
                result.zone_spec.get("acceptance_policy", {}),
            )
            _write(terrain_dir / "visual_acceptance_report.json", visual)
            for capture_script in ("capture_zone_vista.gd", "capture_zone_objective.gd"):
                capture_command = [
                    executable, "--path", str(project_root), "--rendering-driver", "opengl3",
                    "--script", "res://pipeline/" + capture_script, "--",
                    "--batch", batch_relative, "--scene", candidate_scene_resource,
                ]
                _run([xvfb, "-a", *capture_command] if xvfb else capture_command, project_root)
            runtime_capture_command = [
                executable, "--path", str(project_root), "--rendering-driver", "opengl3",
                "--script", "res://pipeline/capture_runtime_effects.gd", "--",
                "--batch", batch_relative, "--scene", candidate_scene_resource,
            ]
            _run(
                [xvfb, "-a", *runtime_capture_command]
                if xvfb
                else runtime_capture_command,
                project_root,
            )
            runtime_effects_acceptance = json.loads(
                (terrain_dir / "runtime_effects_acceptance_report.json").read_text(
                    encoding="utf-8"
                )
            )
            perspective = evaluate_perspective(result.zone_spec, _image(terrain_dir / "godot_zone_vista.png"), _image(terrain_dir / "godot_objective.png"))
            _write(terrain_dir / "perspective_acceptance_report.json", perspective)
            semantic = evaluate_zone(
                result.zone_spec, terrain.manifest, build_report, visual,
                asset_preflight, perspective, overview_projection,
                traversal_report=traversal,
                navigation_report=navigation,
                runtime_effects_report=runtime_effects_acceptance,
            )
            _write(terrain_dir / "acceptance_report.json", semantic)
            if semantic["status"] == "failed":
                reports["status"] = "failed"
                reports["failure"] = "Compiled engine scene failed acceptance"
                reports["stages"].extend(["godot_capture", "overview_projection_acceptance", "navigation_acceptance", "perspective_capture", "runtime_effects_acceptance", "visual_acceptance", "perspective_acceptance", "semantic_acceptance"])
                if preview_active_on_failure:
                    # Development preview mode is explicit and remains red. It
                    # lets an autonomous art loop update the scene already open
                    # in Godot without misrepresenting a failed fidelity gate as
                    # production acceptance.
                    shutil.copy2(candidate_scene_path, active_scene_path)
                    build_report["candidate_scene_path"] = candidate_scene_resource
                    build_report["scene_path"] = "res://moba_3d.tscn"
                    build_report["scene_sha256"] = _sha256(active_scene_path)
                    build_report["promotion_status"] = "preview_promoted_with_failed_acceptance"
                    _write(godot_build_report_path, build_report)
                    reports["stages"].append("active_scene_preview")
                _write(build_report_path, reports)
                raise ZoneCompileError("Compiled engine scene failed acceptance")
            candidate_scene_path.replace(active_scene_path)
            build_report["candidate_scene_path"] = candidate_scene_resource
            build_report["scene_path"] = "res://moba_3d.tscn"
            build_report["scene_sha256"] = _sha256(active_scene_path)
            build_report["promotion_status"] = "promoted_after_acceptance"
            _write(godot_build_report_path, build_report)
            reports["stages"].extend(["godot_capture", "overview_projection_acceptance", "navigation_acceptance", "perspective_capture", "runtime_effects_acceptance", "visual_acceptance", "perspective_acceptance", "semantic_acceptance"])
            reports["stages"].append("active_scene_promotion")
    if capture:
        reports["status"] = "passed"
        reports["acceptance_status"] = "passed"
    elif run_godot:
        reports["status"] = "candidate_built"
        reports["acceptance_status"] = "not_run"
    else:
        reports["status"] = "artifacts_built"
        reports["acceptance_status"] = "not_run"
    _write(build_report_path, reports)
    build_marker_path.unlink(missing_ok=True)
    return reports


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build every derived artifact for a reviewed Codeweald concept batch")
    parser.add_argument("annotations", type=Path)
    parser.add_argument("--project-root", type=Path, default=Path("."))
    parser.add_argument("--asset-root", type=Path, default=Path("assets"))
    parser.add_argument("--godot", action="store_true", help="compile a Godot candidate scene without promoting it")
    parser.add_argument("--capture", action="store_true", help="capture, accept, and atomically promote the Godot candidate; implies --godot")
    parser.add_argument("--godot-bin", default="godot")
    parser.add_argument("--blender-bin", default="blender")
    parser.add_argument("--cross-engine-handoffs", action="store_true", help="emit dormant Unity/Unreal compatibility manifests")
    parser.add_argument(
        "--preview-active-on-failure",
        action="store_true",
        help="overwrite moba_3d.tscn with the best measured candidate even when acceptance remains red",
    )
    parser.add_argument(
        "--catalog-path",
        type=Path,
        help="where to write the asset catalog; defaults inside the engine tree. "
             "Point it at the sandbox when the engine is shared or read-only.",
    )
    args = parser.parse_args(argv)
    try:
        report = build(
            args.annotations,
            args.project_root,
            args.asset_root,
            run_godot=args.godot or args.capture,
            capture=args.capture,
            godot_bin=args.godot_bin,
            blender_bin=args.blender_bin,
            cross_engine_handoffs=args.cross_engine_handoffs,
            preview_active_on_failure=args.preview_active_on_failure,
            catalog_path=args.catalog_path,
        )
    except (OSError, ValueError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Zone build %s: %s" % (report["zone_id"], ", ".join(report["stages"])))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
