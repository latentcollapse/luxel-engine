"""Reachability manifest: does every advertised parameter reach its artifact?

Luxel tooling item 2 (see Luxel/docs/platform/tooling-upgrades.md). Three no-op defects
landed in a single day -- composition scalars the rasterizer never read,
material textures the shader admitted only on steep faces, a generator with
zero callers -- and every other signal stayed green through all of them: the
DSL validated, the spec serialised, the build succeeded, the world was
playable. The only evidence was an artifact that did not move.

``luxel_critic.detect_no_ops`` catches this after the fact, by diffing two
consecutive ``terrain_manifest.json`` builds, and only for scalars the
manifest happens to record. This module makes the check active and total:
every parameter *declares* which artifact digest it claims to control, then
the sweep perturbs each one in turn and asserts that digest moves. A
parameter that cannot move its own artifact is a defect, reported by name
with an owner. No rendering, no judgement, no waiting for a human to notice.

Deliberately checks per (parameter, profile) rather than per parameter: the
rasterizer branches on landform profile, so a scalar can be correctly wired
for ``alpine_jagged_massif`` and dead for ``scattered_crag_field``. Collapsing
those into one verdict would let half a defect hide behind the working half.
"""

from __future__ import annotations

import copy
import hashlib
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from zone_rasterizer import MAX_SPINE_COUNT, rasterize_zone_spec

# Full production terrain is 1025. The sweep rasterises once per
# (feature, parameter) pair, so it trades resolution for turnaround --
# reachability only asks "did this move the field at all", and each
# perturbation is driven to the far end of the parameter's own bounds so a
# genuinely-wired knob moves the field unmistakably even at coarse
# resolution. Raise it if a parameter is ever suspected of an effect too
# small to survive downsampling.
SWEEP_RESOLUTION = 257


@dataclass(frozen=True)
class ParameterDeclaration:
    """One advertised parameter and the artifact it claims to control."""

    name: str
    artifact: str
    owner: str
    # Maps the authored value to a materially different in-bounds value.
    perturb: Callable[[Any], Any]


def _to_far_bound(low: float, high: float) -> Callable[[Any], Any]:
    """Drive a scalar to whichever bound is further from where it sits."""

    def perturb(value: Any) -> float:
        current = float(value)
        return low if abs(current - low) > abs(current - high) else high

    return perturb


def _spine_perturb(value: Any) -> int:
    current = int(value)
    return 1 if current > (1 + MAX_SPINE_COUNT) // 2 else MAX_SPINE_COUNT


# Every scalar the WorldBuilder DSL advertises as authorable composition.
# Adding a knob to the DSL without adding it here is itself the defect this
# module exists to catch, so the sweep cross-checks this table against the
# spec it is given and reports anything authored but undeclared.
DECLARATIONS: tuple[ParameterDeclaration, ...] = (
    ParameterDeclaration(
        name="spine_count",
        artifact="heightfield",
        owner="pipeline/zone_rasterizer.py",
        perturb=_spine_perturb,
    ),
    ParameterDeclaration(
        name="along_jitter",
        artifact="heightfield",
        owner="pipeline/zone_rasterizer.py",
        perturb=_to_far_bound(0.0, 0.5),
    ),
    ParameterDeclaration(
        name="cross_jitter",
        artifact="heightfield",
        owner="pipeline/zone_rasterizer.py",
        perturb=_to_far_bound(0.0, 0.5),
    ),
    ParameterDeclaration(
        name="elevation_bias",
        artifact="heightfield",
        owner="pipeline/zone_rasterizer.py",
        perturb=_to_far_bound(0.0, 1.0),
    ),
)

_DECLARED_NAMES = frozenset(declaration.name for declaration in DECLARATIONS)

# Composition keys that are not numeric scalars controlling the heightfield.
# Listed explicitly so the undeclared-parameter check can tell "categorical,
# belongs to another artifact" apart from "someone added a knob and forgot".
_NON_SCALAR_COMPOSITION_KEYS = frozenset(
    {"pattern", "silhouette", "massing", "surface", "dressing"}
)


@dataclass
class ReachabilityResult:
    parameter: str
    profile: str
    artifact: str
    owner: str
    reachable: bool
    features_tested: list[str] = field(default_factory=list)
    # Landforms excluded from the verdict because the control check showed
    # they cannot move the artifact under any parameter value.
    inconclusive_features: list[str] = field(default_factory=list)

    @property
    def conclusive(self) -> bool:
        return bool(self.features_tested)

    def describe(self) -> str:
        if not self.conclusive:
            return (
                f"{self.parameter} ({self.profile}) INCONCLUSIVE -- no landform "
                f"of this profile affects the {self.artifact} at all, so this "
                "sweep could not have detected a live wire either way "
                f"(skipped: {', '.join(self.inconclusive_features)})"
            )
        verdict = "reaches" if self.reachable else "DOES NOT REACH"
        return (
            f"{self.parameter} ({self.profile}) {verdict} {self.artifact} "
            f"[{self.owner}]"
        )


def heightfield_digest(zone_spec: dict[str, Any], resolution: int) -> str:
    """The artifact digest the composition scalars claim to control."""
    raster = rasterize_zone_spec(zone_spec, resolution=resolution)
    return hashlib.sha256(raster.height_m.tobytes()).hexdigest()


def _landform_features(zone_spec: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        feature
        for feature in zone_spec.get("features", [])
        if isinstance(feature, dict)
        and feature.get("category") == "landform"
        and isinstance(feature.get("generation"), dict)
    ]


def _feature_moves_artifact(
    zone_spec: dict[str, Any],
    feature: dict[str, Any],
    baseline: str,
    resolution: int,
) -> bool:
    """Can this landform move the heightfield at all, under any authoring?

    Perturbs ``elevation_m`` -- the landform's most load-bearing input, and
    not one of the parameters under test -- so a negative answer means the
    landform itself contributes nothing, independent of any scalar's wiring.
    """
    candidate = copy.deepcopy(zone_spec)
    for other in _landform_features(candidate):
        if other.get("id") == feature.get("id"):
            elevation = other["generation"].get("elevation_m")
            if not isinstance(elevation, list) or len(elevation) != 2:
                return False
            floor, peak = float(elevation[0]), float(elevation[1])
            other["generation"]["elevation_m"] = [floor, peak + max(50.0, peak)]
            break
    else:
        return False
    return heightfield_digest(candidate, resolution) != baseline


def undeclared_parameters(zone_spec: dict[str, Any]) -> list[str]:
    """Authored composition scalars that no declaration covers.

    A knob nobody declared is a knob nobody sweeps, which is how a parameter
    goes decorative without anything noticing.
    """
    seen: set[str] = set()
    for feature in _landform_features(zone_spec):
        composition = feature["generation"].get("composition", {})
        if not isinstance(composition, dict):
            continue
        for key, value in composition.items():
            if key in _NON_SCALAR_COMPOSITION_KEYS or key in _DECLARED_NAMES:
                continue
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                continue
            seen.add(key)
    return sorted(seen)


def sweep(
    zone_spec: dict[str, Any], *, resolution: int = SWEEP_RESOLUTION
) -> list[ReachabilityResult]:
    """Perturb every declared parameter and record whether its artifact moved.

    A parameter counts as reaching its artifact for a profile if perturbing it
    on *any* landform of that profile changes the digest. One landform is
    enough to prove the wire exists; it takes every landform failing to prove
    it does not.
    """
    baseline = heightfield_digest(zone_spec, resolution)
    features = _landform_features(zone_spec)

    # (parameter, profile) -> [(feature_id, moved)]
    observations: dict[tuple[str, str], list[tuple[str, bool]]] = {}
    skipped: dict[tuple[str, str], list[str]] = {}
    for feature in features:
        profile = feature["generation"].get("profile") or "unknown"
        composition = feature["generation"].get("composition", {})
        if not isinstance(composition, dict):
            continue
        # Control check. A landform that cannot move the heightfield under
        # ANY value -- a degenerate polygon, a mask that falls between sample
        # points, a zero elevation delta -- makes every parameter on it look
        # dead. Reporting that as "unreachable" would send someone hunting a
        # broken wire in the rasterizer when the landform is what is broken,
        # so it is reported as inconclusive instead. The sweep has to be able
        # to say "I could not have detected this either way".
        if not _feature_moves_artifact(zone_spec, feature, baseline, resolution):
            for declaration in DECLARATIONS:
                if declaration.name in composition:
                    skipped.setdefault((declaration.name, profile), []).append(
                        str(feature.get("id"))
                    )
            continue
        for declaration in DECLARATIONS:
            if declaration.name not in composition:
                continue
            authored = composition[declaration.name]
            perturbed_value = declaration.perturb(authored)
            if perturbed_value == authored:
                # Nothing to learn from a perturbation that changed nothing.
                continue
            candidate = copy.deepcopy(zone_spec)
            for other in _landform_features(candidate):
                if other.get("id") == feature.get("id"):
                    other["generation"]["composition"][declaration.name] = (
                        perturbed_value
                    )
                    break
            moved = heightfield_digest(candidate, resolution) != baseline
            observations.setdefault((declaration.name, profile), []).append(
                (str(feature.get("id")), moved)
            )

    declarations_by_name = {d.name: d for d in DECLARATIONS}
    results: list[ReachabilityResult] = []
    for key in sorted(set(observations) | set(skipped)):
        parameter, profile = key
        entries = observations.get(key, [])
        declaration = declarations_by_name[parameter]
        results.append(
            ReachabilityResult(
                parameter=parameter,
                profile=profile,
                artifact=declaration.artifact,
                owner=declaration.owner,
                reachable=any(moved for _, moved in entries),
                features_tested=[fid for fid, _ in entries],
                inconclusive_features=sorted(skipped.get(key, [])),
            )
        )
    return results


def unreachable(results: list[ReachabilityResult]) -> list[ReachabilityResult]:
    """Only conclusive failures. An inconclusive sweep is not evidence of a
    dead wire, and reporting it as one is how a tool starts lying."""
    return [
        result for result in results if result.conclusive and not result.reachable
    ]


def inconclusive(results: list[ReachabilityResult]) -> list[ReachabilityResult]:
    return [result for result in results if not result.conclusive]


# --- Extension: dead code paths -------------------------------------------
#
# "A generator no build stage can reach is the same defect one level up."
# ``comfy_generate_texture.py`` sat in the tree with zero callers anywhere;
# nothing failed, because nothing ran it.

_SOURCE_SUFFIXES = (".py", ".gd", ".md", ".json", ".toml")

# Generated or local-only trees (all git-ignored). Nothing in them is a wire a
# reader could follow, and `artifacts/` alone is ~12 GB with 140+ scene packets
# over 10 MB: scanning it once per module made the real-pipeline test run for
# hours (found 2026-10-06).
_SKIPPED_DIRS = frozenset({
    "artifacts", "asset_intake_reports", "graphify-out", "kvfold", ".freebuff",
    "target", "__pycache__", ".git", ".pytest_cache", "node_modules",
})


def _source_files(root: Path):
    """Source-suffixed files under `root`, never descending into `_SKIPPED_DIRS`."""
    for directory, subdirectories, files in os.walk(root):
        subdirectories[:] = [name for name in subdirectories if name not in _SKIPPED_DIRS]
        for name in files:
            path = Path(directory) / name
            if path.suffix in _SOURCE_SUFFIXES:
                yield path


@dataclass
class ModuleReachability:
    module: str
    imported_by: list[str]
    referenced_by: list[str]
    is_entry_point: bool

    @property
    def reachable(self) -> bool:
        return bool(self.imported_by or self.referenced_by)

    def describe(self) -> str:
        if self.reachable:
            return f"{self.module}: reachable"
        kind = "entry point" if self.is_entry_point else "module"
        return (
            f"{self.module}: UNREACHABLE {kind} -- nothing imports it and "
            "nothing names it"
        )


def module_reachability(
    pipeline_dir: Path, search_roots: list[Path] | None = None
) -> list[ModuleReachability]:
    """Which pipeline modules can actually be reached, and by what.

    Two distinct ways to be reached, both counted: another module imports it,
    or some file names it (a subprocess invocation, a runbook, a doc). An
    entry point nobody names is as dead as a library nobody imports -- the
    distinction is only reported so a reader knows which kind of wire is
    missing.
    """
    modules = sorted(
        path.stem
        for path in pipeline_dir.glob("*.py")
        if path.stem != "__init__"
    )
    roots = search_roots if search_roots is not None else [pipeline_dir.parent]

    texts: dict[Path, str] = {}
    for root in roots:
        for path in _source_files(root):
            try:
                texts[path] = path.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue

    results: list[ModuleReachability] = []
    for module in modules:
        own_path = pipeline_dir / f"{module}.py"
        source = texts.get(own_path, "")
        is_entry_point = "__main__" in source
        module_name = re.escape(module)
        import_pattern = re.compile(
            rf"\bimport\s+(?:[A-Za-z_]\w*\.)*{module_name}(?:\b|$)"
        )
        from_pattern = re.compile(
            rf"\bfrom\s+(?:[A-Za-z_]\w*\.)*{module_name}\s+import\b"
        )

        imported_by: list[str] = []
        referenced_by: list[str] = []
        for path, text in texts.items():
            if path == own_path:
                continue
            if import_pattern.search(text) or from_pattern.search(text):
                imported_by.append(path.name)
            elif f"{module}.py" in text:
                referenced_by.append(path.name)
        results.append(
            ModuleReachability(
                module=module,
                imported_by=sorted(imported_by),
                referenced_by=sorted(referenced_by),
                is_entry_point=is_entry_point,
            )
        )
    return results


def main(argv: list[str] | None = None) -> int:
    import argparse
    import json as _json

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path, help="compiled concept batch")
    parser.add_argument(
        "--resolution",
        type=int,
        default=SWEEP_RESOLUTION,
        help=f"rasteriser resolution for the sweep (default {SWEEP_RESOLUTION})",
    )
    parser.add_argument(
        "--skip-modules",
        action="store_true",
        help="skip the dead-module extension, sweep parameters only",
    )
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    arguments = parser.parse_args(argv)

    zone_spec_path = arguments.batch / "zone_spec.json"
    if not zone_spec_path.is_file():
        parser.error(f"{zone_spec_path} does not exist; compile the batch first")
    zone_spec = _json.loads(zone_spec_path.read_text(encoding="utf-8"))

    undeclared = undeclared_parameters(zone_spec)
    results = sweep(zone_spec, resolution=arguments.resolution)
    dead_modules: list[ModuleReachability] = []
    if not arguments.skip_modules:
        pipeline_dir = Path(__file__).resolve().parent
        dead_modules = [
            entry
            for entry in module_reachability(pipeline_dir)
            if not entry.reachable
        ]

    failures = unreachable(results)
    if arguments.json:
        print(
            _json.dumps(
                {
                    "schema_version": "codeweald.reachability/v1",
                    "undeclared_parameters": undeclared,
                    "parameters": [
                        {
                            "parameter": r.parameter,
                            "profile": r.profile,
                            "artifact": r.artifact,
                            "owner": r.owner,
                            "reachable": r.reachable,
                            "conclusive": r.conclusive,
                            "features_tested": r.features_tested,
                            "inconclusive_features": r.inconclusive_features,
                        }
                        for r in results
                    ],
                    "unreachable_modules": [
                        {"module": m.module, "is_entry_point": m.is_entry_point}
                        for m in dead_modules
                    ],
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        for result in results:
            if not result.conclusive:
                marker = "????"
            elif result.reachable:
                marker = "ok  "
            else:
                marker = "FAIL"
            print(f"[{marker}] {result.describe()}")
        for entry in dead_modules:
            print(f"[FAIL] {entry.describe()}")
        for name in undeclared:
            print(
                f"[FAIL] {name}: authored composition scalar with no "
                "reachability declaration (pipeline/reachability.py)"
            )
    return 0 if not (failures or dead_modules or undeclared) else 2


if __name__ == "__main__":
    raise SystemExit(main())
