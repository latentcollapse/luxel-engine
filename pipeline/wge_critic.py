#!/usr/bin/env python3
"""Turn audit metrics into concrete, applicable WorldBuilder repairs.

The audit suite reports numbers: `foreground_edge_density: 0.044451`. That is
legible to a person who already knows the pipeline and useless to everyone
else, including any model asked to improve the world.

This module closes that gap. It compares the render against the source art,
states each difference in world language rather than metric language, and --
where the authoring DSL can actually express the fix -- emits a complete,
valid intent file with the repair already applied.

It is deliberately honest about the boundary: some defects are not
DSL-addressable (foliage budget, hydrology, materials). Those are reported as
diagnoses with an owner, never as silent no-ops, so nobody burns a loop
editing a knob that cannot move the metric.
"""
from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from metric_honesty import labels as metric_labels  # noqa: E402
from worldbuilder_dsl import (  # noqa: E402
    _PATTERN_SCALARS,
    _SCALAR_BOUNDS,
    _SEMANTIC_COMPOSITIONS,
    WorldBuilderError,
    scaffold_intent,
)

CRITIC_VERSION = "codeweald.wge-critic/v1"

# Hard gates enforced by bevy_visual_acceptance. Below these the build fails
# outright; these are floors, not quality targets.
_GATES = {
    "foreground_edge_density": 0.008,
    "foreground_dynamic_range": 0.10,
    "foliage_fraction": 0.008,
}

# How close to the source art a metric should land before we stop nagging.
# A render never matches painted concept art exactly; 0.65 of source edge
# density is "reads like the same place", not "is the same image".
_FIDELITY_TARGET = 0.65

# Largest single-pass multiplier on a scalar. Repairs should converge over a
# few audit cycles rather than slamming a value to its bound and overshooting.
_MAX_STEP = 1.8


@dataclass
class Finding:
    """One difference between the world we built and the world we wanted."""

    metric: str
    observed: float
    target: float
    severity: str  # "gate" | "fidelity"
    diagnosis: str  # stated in world language
    actionable: bool
    owner: str = "worldbuilder-dsl"
    patches: dict[str, dict[str, float]] = field(default_factory=dict)

    @property
    def ratio(self) -> float:
        return self.observed / self.target if self.target else 0.0

    @property
    def metric_kind(self) -> str:
        """What the driving metric actually measures (tooling item 6).

        This matters most exactly where it is least visible. `detail_density`
        and `foreground_edge_density` are *coverage gates*: measured against
        adversarial controls, both score an un-filtered render roughly 8x and
        2.5x above the correctly filtered one. So a finding that says "raise
        detail_density toward the source art" can be satisfied by making the
        renderer alias more -- an incentive to degrade the instrument in order
        to pass the gate. Carrying the kind alongside the target is what stops a
        reader, or an automated repair loop, taking the number for a fidelity
        judgement.
        """
        return metric_labels().get(self.metric, "unclassified")

    def describe(self) -> str:
        head = (
            f"[{self.severity}] {self.diagnosis}\n"
            f"    {self.metric}: {self.observed:.6g} vs target {self.target:.6g}"
        )
        if self.metric_kind == "coverage_gate":
            # Say it here rather than only in a report nobody opens: this
            # target can be satisfied by aliasing the render harder.
            head += (
                "\n    NOTE: this metric is a coverage gate, not a quality"
                " score -- an un-filtered render scores higher on it than a"
                " correct one. Treat it as a floor, never as a fidelity target."
            )
        if self.observed and self.target:
            head += f"  ({self.target / self.observed:.1f}x short)"
        if not self.actionable:
            return head + f"\n    not fixable from the DSL -- owned by: {self.owner}"
        if self.patches:
            edits = "; ".join(
                f"{fid}: " + ", ".join(f"{k}->{v:g}" for k, v in sorted(chg.items()))
                for fid, chg in sorted(self.patches.items())
            )
            head += f"\n    suggested: {edits}"
        return head


def _landforms(zone_spec: dict[str, Any]) -> dict[str, dict]:
    return {
        f["id"]: f
        for f in zone_spec.get("features", [])
        if isinstance(f, dict)
        and f.get("category") == "landform"
        and str(f.get("semantic")) in _SEMANTIC_COMPOSITIONS
        and isinstance(f.get("id"), str)
    }


def _current(feature: dict, key: str) -> float:
    """Current value of a composition scalar, falling back to the pattern default."""
    comp = feature.get("generation", {}).get("composition") or {}
    if key == "spines":
        pattern = comp.get("pattern") or "ridge_network"
        return float(comp.get("spine_count", _PATTERN_SCALARS[pattern]["spines"]))
    pattern = comp.get("pattern") or "ridge_network"
    return float(comp.get(key, _PATTERN_SCALARS[pattern][key]))


def _scaled(value: float, factor: float, key: str) -> float:
    lo, hi = _SCALAR_BOUNDS[key]
    return round(min(hi, max(lo, value * factor)), 4)


def _detail_pair(render: dict, source: dict) -> tuple[float, float, bool]:
    """Render and source surface detail, and whether they are comparable.

    ``detail_density`` is computed by one function at one resolution with one
    threshold over one denominator, so the two sides can be divided. The legacy
    pair cannot: ``foreground_edge_density`` averages over foreground pixels at
    threshold 0.045 while ``edge_density`` averages over the whole frame at
    0.055, which inflates the render against its own target by an unknown
    amount. Reports fall back to it only so an old capture still says something,
    and say that they did.
    """
    detail = render.get("detail_density")
    src_detail = source.get("detail_density")
    if detail is not None and src_detail is not None:
        return float(detail), float(src_detail), True
    return (
        float(render.get("foreground_edge_density", 0.0)),
        float(source.get("edge_density", 0.0)),
        False,
    )


def diagnose(zone_spec: dict, render: dict, source: dict) -> list[Finding]:
    """Compare a render against its source art and the hard gates."""
    findings: list[Finding] = []
    landforms = _landforms(zone_spec)

    # --- surface detail ---------------------------------------------------
    # Measured 2026-07-31 across three build+capture cycles: driving every
    # landform's spine_count to 8 and both jitters to 0.30 moved this metric
    # from 0.044451 to 0.044351 -- 0.3% against a 6.9x shortfall -- while
    # pushing the jitters to 0.5 broke the traversability gate outright. The
    # scalars have real geometric authority and none over this number.
    #
    # The reason is in the metric itself: it thresholds a per-pixel luminance
    # gradient, so it counts texture, material break-up, and shading. Painted
    # concept art carries detail on every rock face; a flat-shaded heightfield
    # under a four-channel splatmap carries it only along ridgelines. This is a
    # materials gap wearing a geometry-shaped name, and it is reported against
    # the owner that can actually close it.
    #
    # Silhouette complexity is a real and separate property. Nothing here
    # measures it yet; when a skyline metric exists it can drive composition
    # honestly, which this never could.
    detail, src_detail, comparable = _detail_pair(render, source)
    detail_target = src_detail * _FIDELITY_TARGET
    if src_detail and detail < detail_target:
        findings.append(
            Finding(
                metric="detail_density" if comparable else "foreground_edge_density",
                observed=detail,
                target=detail_target,
                severity=(
                    "gate"
                    if render.get("foreground_edge_density", 1.0)
                    < _GATES["foreground_edge_density"]
                    else "fidelity"
                ),
                diagnosis=(
                    "The world reads far flatter and cleaner than the concept art. "
                    "The source carries fractured rock texture, weathering, and "
                    "shading detail across every surface; ours is smooth material "
                    "over smooth terrain. This is surface treatment, not landform "
                    "shape -- rebuilding the same world with every composition "
                    "scalar at its bound moves this metric by well under one "
                    "percent."
                    + (
                        ""
                        if comparable
                        else " (Comparing legacy per-side metrics: this render "
                        "predates detail_density, so the shortfall is only "
                        "approximate. Recapture to get a real number.)"
                    )
                ),
                actionable=False,
                owner="terrain materials / renderer shading",
            )
        )

    # --- relief depth -----------------------------------------------------
    dyn = float(render.get("foreground_dynamic_range", 0.0))
    src_dyn = float(source.get("luminance_stddev", 0.0)) * 2.0
    if src_dyn and dyn < src_dyn * _FIDELITY_TARGET:
        factor = min(_MAX_STEP, (src_dyn * _FIDELITY_TARGET / dyn) ** 0.5) if dyn else _MAX_STEP
        findings.append(
            Finding(
                metric="foreground_dynamic_range",
                observed=dyn,
                target=src_dyn * _FIDELITY_TARGET,
                severity="gate" if dyn < _GATES["foreground_dynamic_range"] else "fidelity",
                diagnosis=(
                    "The landforms are too uniform in height -- the world lacks the "
                    "light-to-dark relief the source art gets from real elevation change."
                ),
                actionable=True,
                patches={
                    fid: {
                        "elevation_bias": _scaled(
                            _current(f, "elevation_bias"), factor, "elevation_bias"
                        )
                    }
                    for fid, f in landforms.items()
                },
            )
        )

    # --- defects the DSL cannot reach ------------------------------------
    foliage = float(render.get("foliage_fraction", 0.0))
    if foliage < _GATES["foliage_fraction"] * 2:
        findings.append(
            Finding(
                metric="foliage_fraction",
                observed=foliage,
                target=_GATES["foliage_fraction"] * 2,
                severity="gate" if foliage < _GATES["foliage_fraction"] else "fidelity",
                diagnosis=(
                    "Vegetation is sparse enough that the world reads as bare rock. "
                    "Foliage is solved by the compiler's canopy budget, not by landform "
                    "composition."
                ),
                actionable=False,
                owner="pipeline/zone_rasterizer.py (canopy budget)",
            )
        )

    dark = float(render.get("dark_foreground_fraction", 0.0))
    if dark <= 0.0:
        findings.append(
            Finding(
                metric="dark_foreground_fraction",
                observed=dark,
                target=0.02,
                severity="fidelity",
                diagnosis=(
                    "Nothing in the frame is genuinely dark. The source art uses deep "
                    "shadow in gullies and north faces to read as mountainous; ours is "
                    "evenly lit, which flattens every form."
                ),
                actionable=False,
                owner="terrain materials / renderer shading",
            )
        )

    return findings


def detect_no_ops(previous: dict, current: dict) -> list[Finding]:
    """Report authoring scalars that changed without changing the terrain.

    This is the general no-op check, not a guard against one known bug. Any
    parameter this pipeline advertises as authorable can silently stop being
    wired -- to a new profile, a new backend, a refactor -- and every other
    signal stays green while it happens: the DSL validates, the spec
    serializes, the build succeeds, the world is playable. The only evidence is
    that the heightfield did not move.

    Two terrain manifests are enough to see it. If the composition scalars
    differ between builds and ``heightfield_sha256`` does not, the parameters
    that changed are non-actionable, and saying so costs one comparison instead
    of an afternoon of hand-tuning a decorative knob.
    """
    if not previous or not current:
        return []
    before_hash = previous.get("heightfield_sha256")
    after_hash = current.get("heightfield_sha256")
    if not before_hash or not after_hash or before_hash != after_hash:
        return []

    def scalars(manifest: dict) -> dict[str, dict]:
        return {
            entry["id"]: entry.get("composition_scalars") or {}
            for entry in manifest.get("landforms", [])
            if isinstance(entry, dict) and isinstance(entry.get("id"), str)
        }

    before, after = scalars(previous), scalars(current)
    inert: dict[str, list[str]] = {}
    for fid, changed in after.items():
        if fid not in before:
            continue
        # Only keys the previous manifest also recorded. A key appearing for
        # the first time has no before-value to differ from, and treating its
        # absence as a change reports a schema addition as a dead knob:
        # recording elevation_bias in the manifest for the first time made
        # this fire against a scalar the sensitivity matrix measures at -57.7%
        # authority. A detector that cries wolf on its own upgrades is worse
        # than one that stays quiet.
        moved = sorted(
            key
            for key, value in changed.items()
            if key in before[fid] and before[fid][key] != value
        )
        if moved:
            inert[fid] = moved
    if not inert:
        return []

    named = sorted({key for keys in inert.values() for key in keys})
    where = "; ".join(f"{fid}: {', '.join(keys)}" for fid, keys in sorted(inert.items()))
    return [
        Finding(
            metric="composition_no_op",
            observed=0.0,
            target=float(len(named)),
            severity="gate",
            diagnosis=(
                "The last build changed authoring parameters and produced the "
                "byte-identical terrain. These parameters do not reach the "
                f"geometry they claim to control: {', '.join(named)}. Editing "
                "them will keep doing nothing until they are wired into the "
                f"rasterizer ({where})."
            ),
            actionable=False,
            owner="pipeline/zone_rasterizer.py (composition scalars not wired)",
        )
    ]


def apply_sensitivity_matrix(
    findings: list[Finding], matrix_document: dict | None
) -> list[Finding]:
    """Let measured authority override the critic's hardcoded attribution.

    Tooling item 3. Without this, which knob "fixes" which metric is a guess
    baked into the code above -- and one such guess sent the loop emitting
    jitter/spine repairs against a metric those scalars move by 0.3%. A repair
    that cannot move its target is worse than no repair: it looks like
    progress and consumes iterations indefinitely.

    Demotes, never promotes. A metric the matrix says nobody owns becomes
    non-actionable and its patches are dropped; a metric with owners keeps
    whatever the hand-written rule proposed, minus any patch naming a knob
    the measurement says has no authority over it. Promotion would mean
    inventing a repair from a correlation, which is how a loop starts chasing
    coincidences.
    """
    if not matrix_document:
        return findings
    owners = matrix_document.get("metric_owners") or {}
    # A metric the sweep could not measure is not a metric nobody owns.
    # Demoting on an absent measurement would silence real findings on no
    # evidence, which is the same error as trusting a fabricated one.
    unmeasurable = set(matrix_document.get("unmeasurable_metrics") or [])
    for finding in findings:
        if not finding.actionable or finding.metric not in owners:
            continue
        if finding.metric in unmeasurable:
            continue
        owning_knobs = set(owners[finding.metric])
        if not owning_knobs:
            finding.actionable = False
            finding.patches = {}
            finding.owner = (
                f"no DSL owner -- measured: no authoring knob moves "
                f"{finding.metric} by more than "
                f"{matrix_document.get('authority_threshold', 0.02):.0%} "
                "(pipeline/sensitivity.py)"
            )
            continue
        trimmed = {
            fid: {k: v for k, v in changes.items() if k in owning_knobs}
            for fid, changes in finding.patches.items()
        }
        finding.patches = {fid: chg for fid, chg in trimmed.items() if chg}
        if finding.patches:
            continue
        # Every knob the rule proposed was measured powerless over this
        # metric, so the rule was pointed at the wrong lever entirely.
        finding.actionable = False
        finding.owner = (
            f"misattributed -- the proposed knobs do not move {finding.metric}; "
            f"measured owners: {', '.join(sorted(owning_knobs))} "
            "(pipeline/sensitivity.py)"
        )
    return findings


def detect_unreachable_parameters(zone_spec: dict) -> list[Finding]:
    """Active reachability sweep, reported in the critic's own vocabulary.

    Where ``detect_no_ops`` waits for a build to reveal an inert knob, this
    perturbs each declared parameter itself and asserts the artifact it claims
    to control actually moves. Findings are non-actionable by construction: a
    dead wire is fixed in the stage that owns it, never by editing the DSL
    value that is failing to reach it.
    """
    from reachability import sweep, unreachable

    dead = unreachable(sweep(zone_spec))
    findings: list[Finding] = []
    for result in dead:
        findings.append(
            Finding(
                metric="parameter_unreachable",
                observed=0.0,
                target=1.0,
                severity="gate",
                diagnosis=(
                    f"'{result.parameter}' is authorable on {result.profile} "
                    f"landforms but does not reach the {result.artifact} it "
                    "claims to control: driving it to the far end of its own "
                    "bounds left the artifact byte-identical. Editing it will "
                    "keep doing nothing until it is wired "
                    f"(landforms tested: {', '.join(result.features_tested)})."
                ),
                actionable=False,
                owner=result.owner,
            )
        )
    return findings


def merge_patches(findings: list[Finding]) -> dict[str, dict[str, float]]:
    """Combine per-finding edits. Later findings win on a shared key."""
    merged: dict[str, dict[str, float]] = {}
    for finding in findings:
        if not finding.actionable:
            continue
        for fid, change in finding.patches.items():
            merged.setdefault(fid, {}).update(change)
    return merged


def apply_to_scaffold(zone_spec: dict, patches: dict[str, dict[str, float]]) -> str:
    """Emit a complete intent file with the repairs already applied.

    Routed through scaffold_intent so the output is a full, valid file by
    construction rather than a hand-built fragment that might not compile.
    """
    import re

    source = scaffold_intent(zone_spec)
    out, current = [], None
    applied: set[tuple[str, str]] = set()
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("'") and stripped.endswith("',"):
            current = stripped[1:-2]
        if current in patches:
            for key, value in patches[current].items():
                if re.match(rf"\s+{key}=", line):
                    rendered = int(value) if key == "spines" else value
                    line = re.sub(rf"({key}=)[^,]+", rf"\g<1>{rendered}", line)
                    applied.add((current, key))
        out.append(line)
    # A patch that matched no line used to vanish without a word, which makes
    # a repair that does nothing indistinguishable from one that worked. The
    # authoring surface names the spine count `spines` while the ZoneSpec,
    # manifest and rasterizer all call it `spine_count`, so this is a live
    # trap rather than a hypothetical one -- it silently swallowed a whole
    # row of the first sensitivity matrix.
    requested = {
        (feature_id, key)
        for feature_id, changes in patches.items()
        for key in changes
    }
    dropped = sorted(requested - applied)
    if dropped:
        listed = "; ".join(f"{fid}.{key}" for fid, key in dropped)
        raise ValueError(
            "these patches matched nothing in the generated intent and would "
            f"have been silently discarded: {listed}. The authoring surface "
            "names the spine count 'spines', not 'spine_count'."
        )
    return "\n".join(out) + "\n"


def load_reports(batch: Path) -> tuple[dict, dict, dict]:
    zone_spec = json.loads((batch / "zone_spec.json").read_text())
    render = json.loads((batch / "bevy_visual_acceptance_report.json").read_text())
    style = json.loads((batch / "style_reference.json").read_text())
    return zone_spec, render.get("metrics", {}), style.get("metrics", {})


def load_terrain_manifests(batch: Path) -> tuple[dict, dict]:
    """The previous and current terrain manifests, empty when unavailable.

    A first build has nothing to compare against; that is not an error, it just
    means the no-op check has no evidence yet.
    """

    def read(name: str) -> dict:
        path = batch / "terrain" / name
        try:
            loaded = json.loads(path.read_text())
        except (OSError, ValueError):
            return {}
        return loaded if isinstance(loaded, dict) else {}

    return read("terrain_manifest.previous.json"), read("terrain_manifest.json")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Diagnose a built world and emit DSL repairs for it"
    )
    parser.add_argument("batch", type=Path, help="concept batch directory")
    parser.add_argument("--emit-intent", type=Path, help="write a repaired intent file here")
    parser.add_argument("--json", action="store_true", help="machine-readable findings")
    parser.add_argument(
        "--reachability",
        action="store_true",
        help=(
            "actively perturb every declared authoring parameter and verify it "
            "moves the artifact it claims to control (slow: rasterises once per "
            "parameter/landform pair)"
        ),
    )
    args = parser.parse_args(argv)

    try:
        zone_spec, render, source = load_reports(args.batch)
    except (OSError, ValueError) as exc:
        print(f"{args.batch}: {exc}", file=sys.stderr)
        return 2

    # A no-op is reported before the fidelity findings: if the knobs are inert,
    # every repair below it is advice that cannot be taken.
    previous_terrain, current_terrain = load_terrain_manifests(args.batch)
    findings = detect_no_ops(previous_terrain, current_terrain) + diagnose(
        zone_spec, render, source
    )
    # Measured authority outranks the hardcoded attribution above, whenever a
    # matrix has been published for this batch.
    from sensitivity import load_matrix

    findings = apply_sensitivity_matrix(findings, load_matrix(args.batch))

    if args.reachability:
        # detect_no_ops is passive: it only sees a knob go inert if a build
        # happened to change that knob. The sweep is active -- it changes every
        # declared knob on purpose -- so it finds a dead wire nobody happened
        # to touch, including one that has never worked at all.
        findings = detect_unreachable_parameters(zone_spec) + findings

    if args.json:
        print(
            json.dumps(
                {
                    "schema_version": CRITIC_VERSION,
                    "zone_id": zone_spec.get("zone", {}).get("id"),
                    "findings": [
                        {
                            "metric": f.metric,
                            "observed": f.observed,
                            "target": f.target,
                            "severity": f.severity,
                            "diagnosis": f.diagnosis,
                            "actionable": f.actionable,
                            "owner": f.owner,
                            "patches": f.patches,
                        }
                        for f in findings
                    ],
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        if not findings:
            print("No findings: the render matches the source within tolerance.")
        for finding in findings:
            print(finding.describe())
            print()

    if args.emit_intent:
        patches = merge_patches(findings)
        if not patches:
            print("Nothing DSL-addressable to emit.", file=sys.stderr)
            return 1
        try:
            args.emit_intent.write_text(apply_to_scaffold(zone_spec, patches), encoding="utf-8")
        except WorldBuilderError as exc:
            print(f"could not scaffold repairs: {exc}", file=sys.stderr)
            return 2
        print(f"Wrote repaired intent for {len(patches)} landforms to {args.emit_intent}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
