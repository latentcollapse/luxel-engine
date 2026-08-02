#!/usr/bin/env python3
"""Sandboxed Python-shaped authoring frontend for Codeweald WorldSpec patches.

The source is parsed, never executed. Only declarative calls in this module's
small vocabulary are interpreted, giving language models familiar Python
syntax without host filesystem, network, process, import, or runtime access.

Syntactic correctness is free for any model that can write Python; semantic
correctness is not. Every rejection here therefore tries to carry an
executable repair, so recovering from a mistake is an edit rather than a
redesign. See ``WorldBuilderError.suggestion``.

A value this module accepts is not the same promise as a value the build
will accept. ``_SCALAR_BOUNDS`` (and the matching scaffold header comments)
describe what is *documented* as legal -- e.g. ``along_jitter``/
``cross_jitter`` in 0.0-0.5 -- not what is *buildable*. A legal value can
still fail a downstream build-time gate (most commonly terrain
accessibility in ``build_zone.py``); that failure names the offending
landform and knob, so treat it as the real bound this module cannot express.
"""

from __future__ import annotations

import argparse
import ast
import copy
import difflib
import json
import sys
from pathlib import Path
from typing import Any, Iterable


INTENT_VERSION = "codeweald.world-intent/v1"
_IMPORTS = {"World", "ridge_network", "clustered_ridges"}
_COMPOSITION_PATTERNS = {
    "ridge_network": "ridge_network",
    "clustered_ridges": "clustered_ridges",
}
_SILHOUETTES = {"continuous_boundary_wall", "broken_ridge_cluster"}
_MASSING = {"terrain_primary", "terrain_and_sparse_props"}
_SURFACES = {"fractured_granite", "weathered_highland_rock"}
_DRESSING = {"none", "sparse", "moderate"}

# Which composition patterns each landform semantic can actually be built from.
# Single source of truth: both the validator and the repair suggestions read it.
_SEMANTIC_COMPOSITIONS = {
    "alpine_massif": {"ridge_network"},
    "alpine_ridge": {"clustered_ridges"},
    "crag_field": {"clustered_ridges"},
}

# Starting values per pattern, chosen to be plausible as-authored rather than
# merely valid. Both the repair suggestions and the scaffolder read these, so a
# suggested patch and a generated file can never disagree.
_PATTERN_SCALARS = {
    "ridge_network": {
        "spines": 3,
        "elevation_bias": 0.72,
        "along_jitter": 0.08,
        "cross_jitter": 0.10,
    },
    "clustered_ridges": {
        "spines": 4,
        "elevation_bias": 0.55,
        "along_jitter": 0.18,
        "cross_jitter": 0.32,
    },
}

_PATTERN_DEFAULTS = {
    "ridge_network": {
        "silhouette": "continuous_boundary_wall",
        "massing": "terrain_primary",
        "surface": "fractured_granite",
        "dressing": "sparse",
    },
    "clustered_ridges": {
        "silhouette": "broken_ridge_cluster",
        "massing": "terrain_and_sparse_props",
        "surface": "weathered_highland_rock",
        "dressing": "sparse",
    },
}

# Paste-ready calls used to build repair text, derived so they cannot drift.
_COMPOSITION_TEMPLATES = {
    pattern: "{}({})".format(
        pattern,
        ", ".join(
            f"{key}={value}" if key == "spines" else f"{key}={value:.2f}"
            for key, value in scalars.items()
        ),
    )
    for pattern, scalars in _PATTERN_SCALARS.items()
}

_SCALAR_BOUNDS = {
    "elevation_bias": (0.0, 1.0),
    "along_jitter": (0.0, 0.5),
    "cross_jitter": (0.0, 0.5),
}


class WorldBuilderError(ValueError):
    """An authoring rejection that carries a repair whenever one exists.

    ``suggestion`` holds valid WorldBuilder DSL text that can be pasted in
    place of the offending code. ``args[0]`` stays the bare message so callers
    can match on it; ``str()`` renders message, line, and repair together.
    """

    def __init__(
        self,
        message: str,
        *,
        suggestion: str | None = None,
        line: int | None = None,
    ) -> None:
        self.message = str(message)
        self.suggestion = suggestion
        self.line = line
        super().__init__(self.message)

    def __str__(self) -> str:
        parts = [self.message]
        if self.line is not None:
            parts.append(f"  at line {self.line}")
        if self.suggestion:
            parts.append(f"  try: {self.suggestion}")
        return "\n".join(parts)


def _nearest(value: Any, choices: Iterable[str]) -> str | None:
    """Closest known spelling of ``value``, or None when nothing is close."""
    if not isinstance(value, str):
        return None
    matches = difflib.get_close_matches(value, sorted(choices), n=1, cutoff=0.6)
    return matches[0] if matches else None


def _did_you_mean(value: Any, choices: Iterable[str]) -> str:
    near = _nearest(value, choices)
    return f" (did you mean {near!r}?)" if near else ""


def _one_of(choices: Iterable[str]) -> str:
    return ", ".join(sorted(choices))


def _landform_call(feature_id: str, feature: dict[str, Any]) -> str:
    """A paste-ready landform call valid for this feature's own semantic."""
    allowed = _SEMANTIC_COMPOSITIONS.get(str(feature.get("semantic")), set())
    pattern = sorted(allowed)[0] if allowed else "ridge_network"
    return (
        f"world.landform({feature_id!r}, "
        f"composition={_COMPOSITION_TEMPLATES[pattern]})"
    )


def _literal(node: ast.AST, repairs: list[str] | None = None) -> Any:
    if isinstance(node, ast.Constant) and isinstance(
        node.value, (str, int, float, bool, type(None))
    ):
        return node.value
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
        value = _literal(node.operand, repairs)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            return -value
    if isinstance(node, (ast.List, ast.Tuple)):
        return [_literal(item, repairs) for item in node.elts]
    if isinstance(node, ast.Dict):
        return {
            _literal(key, repairs): _literal(value, repairs)
            for key, value in zip(node.keys, node.values, strict=True)
        }
    if isinstance(node, ast.Call):
        return _composition_call(node, repairs)
    if isinstance(node, ast.Name):
        raise WorldBuilderError(
            f"{node.id!r} is a variable reference; WorldBuilder values must be "
            "written out literally so the world can be read without running it",
            suggestion=f"replace {node.id} with the value it stands for",
            line=getattr(node, "lineno", None),
        )
    raise WorldBuilderError(
        "WorldBuilder values must be literals (numbers, strings, lists, dicts) "
        f"or one of these composition calls: {_one_of(_COMPOSITION_PATTERNS)}",
        suggestion=_COMPOSITION_TEMPLATES["ridge_network"],
        line=getattr(node, "lineno", None),
    )


def _keywords(call: ast.Call, repairs: list[str] | None = None) -> dict[str, Any]:
    values: dict[str, Any] = {}
    for keyword in call.keywords:
        if keyword.arg is None:
            raise WorldBuilderError(
                "**kwargs expansion is forbidden; write each argument out by name",
                line=getattr(call, "lineno", None),
            )
        if keyword.arg in values:
            raise WorldBuilderError(
                f"duplicate keyword {keyword.arg!r}; give it exactly once",
                line=getattr(call, "lineno", None),
            )
        values[keyword.arg] = _literal(keyword.value, repairs)
    return values


def _composition_call(
    call: ast.Call, repairs: list[str] | None = None
) -> dict[str, Any]:
    line = getattr(call, "lineno", None)
    if not isinstance(call.func, ast.Name) or call.func.id not in _COMPOSITION_PATTERNS:
        name = call.func.id if isinstance(call.func, ast.Name) else None
        near = _nearest(name, _COMPOSITION_PATTERNS)
        raise WorldBuilderError(
            f"{name!r} is not a composition constructor; the available ones are "
            f"{_one_of(_COMPOSITION_PATTERNS)}"
            if name
            else f"composition must be one of: {_one_of(_COMPOSITION_PATTERNS)}",
            suggestion=_COMPOSITION_TEMPLATES[near or "ridge_network"],
            line=line,
        )
    name = call.func.id
    if call.args:
        raise WorldBuilderError(
            f"{name} takes keyword arguments only, so each value is named at the "
            "call site and cannot be silently reordered",
            suggestion=_COMPOSITION_TEMPLATES[name],
            line=line,
        )
    values = _keywords(call, repairs)
    required = {
        "spines",
        "elevation_bias",
        "along_jitter",
        "cross_jitter",
    }
    allowed = required | {"silhouette", "massing", "surface", "dressing"}
    unknown = sorted(set(values) - allowed)
    if unknown:
        hints = "; ".join(f"{arg!r}{_did_you_mean(arg, allowed)}" for arg in unknown)
        raise WorldBuilderError(
            f"{name} got unknown argument(s): {hints}. Accepted arguments are "
            f"{_one_of(allowed)}",
            suggestion=_COMPOSITION_TEMPLATES[name],
            line=line,
        )
    missing = sorted(required - set(values))
    if missing:
        raise WorldBuilderError(
            f"{name} is missing required argument(s): {', '.join(missing)}",
            suggestion=_COMPOSITION_TEMPLATES[name],
            line=line,
        )
    spines = values["spines"]
    if not isinstance(spines, int) or isinstance(spines, bool):
        raise WorldBuilderError(
            f"spines must be a whole number in [1, 8], not {spines!r}",
            suggestion="spines=3",
            line=line,
        )
    if not 1 <= spines <= 8:
        clamped = min(8, max(1, spines))
        if repairs is not None:
            repairs.append(f"clamped spines from {spines} to {clamped}")
            values["spines"] = spines = clamped
        else:
            raise WorldBuilderError(
                f"spines must be in [1, 8], got {spines}",
                suggestion=f"spines={clamped}",
                line=line,
            )
    for key, (lower, upper) in _SCALAR_BOUNDS.items():
        value = values[key]
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise WorldBuilderError(
                f"{key} must be a number in [{lower}, {upper}], not {value!r}",
                suggestion=f"{key}={upper / 2:.2f}",
                line=line,
            )
        if not lower <= float(value) <= upper:
            clamped = min(upper, max(lower, float(value)))
            if repairs is not None:
                repairs.append(f"clamped {key} from {value} to {clamped}")
                values[key] = clamped
            else:
                raise WorldBuilderError(
                    f"{key} must be in [{lower}, {upper}], got {value}",
                    suggestion=f"{key}={clamped}",
                    line=line,
                )
    defaults = dict(_PATTERN_DEFAULTS[name])
    for key, choices in (
        ("silhouette", _SILHOUETTES),
        ("massing", _MASSING),
        ("surface", _SURFACES),
        ("dressing", _DRESSING),
    ):
        value = values.get(key, defaults[key])
        if not isinstance(value, str) or value not in choices:
            near = _nearest(value, choices)
            if near is not None and repairs is not None:
                repairs.append(f"corrected {key} from {value!r} to {near!r}")
                defaults[key] = near
                continue
            raise WorldBuilderError(
                f"{key}={value!r} is not recognised; it must be one of: "
                f"{_one_of(choices)}",
                suggestion=f"{key}={(near or sorted(choices)[0])!r}",
                line=line,
            )
        defaults[key] = value
    return {
        "pattern": _COMPOSITION_PATTERNS[call.func.id],
        "spine_count": spines,
        "elevation_bias": float(values["elevation_bias"]),
        "along_jitter": float(values["along_jitter"]),
        "cross_jitter": float(values["cross_jitter"]),
        **defaults,
    }


def compile_intent(
    source: str,
    *,
    source_name: str = "<world-intent>",
    lenient: bool = False,
) -> dict[str, Any]:
    """Compile Python-shaped world intent into a WorldSpec patch set.

    With ``lenient=True``, recoverable near-misses (out-of-range scalars,
    misspelled enum values) are repaired in place and recorded under the
    ``repairs`` key instead of raising. Structural mistakes still raise.
    Certification stays strict; leniency is an authoring aid only.
    """
    repairs: list[str] | None = [] if lenient else None
    try:
        tree = ast.parse(source, filename=source_name, mode="exec")
    except SyntaxError as exc:
        raise WorldBuilderError(
            f"the intent file is not valid Python: {exc.msg}",
            line=exc.lineno,
        ) from exc
    bindings: dict[str, dict[str, Any]] = {}
    world_id: str | None = None
    patches: list[dict[str, Any]] = []

    for statement in tree.body:
        if isinstance(statement, ast.ImportFrom):
            if (
                statement.module != "worldbuilder"
                or statement.level != 0
                or any(alias.asname is not None or alias.name not in _IMPORTS for alias in statement.names)
            ):
                names = [alias.name for alias in statement.names]
                unknown = [name for name in names if name not in _IMPORTS]
                hints = "".join(_did_you_mean(name, _IMPORTS) for name in unknown)
                raise WorldBuilderError(
                    f"only plain named imports from 'worldbuilder' are allowed, and "
                    f"only these names: {_one_of(_IMPORTS)}{hints}",
                    suggestion="from worldbuilder import World, ridge_network",
                    line=statement.lineno,
                )
            continue
        if isinstance(statement, ast.Assign):
            if len(statement.targets) != 1 or not isinstance(statement.targets[0], ast.Name):
                raise WorldBuilderError(
                    "an assignment must bind exactly one plain name",
                    suggestion='world = World("my_world")',
                    line=statement.lineno,
                )
            if not isinstance(statement.value, ast.Call) or not isinstance(
                statement.value.func, ast.Name
            ) or statement.value.func.id != "World":
                raise WorldBuilderError(
                    "the only value that may be assigned is World(...); every other "
                    "change to the world is a world.landform(...) statement",
                    suggestion='world = World("my_world")',
                    line=statement.lineno,
                )
            call = statement.value
            if len(call.args) != 1 or call.keywords:
                raise WorldBuilderError(
                    "World takes exactly one positional argument, its id string",
                    suggestion='world = World("my_world")',
                    line=statement.lineno,
                )
            identifier = _literal(call.args[0], repairs)
            if not isinstance(identifier, str) or not identifier:
                raise WorldBuilderError(
                    f"the World id must be a non-empty string, not {identifier!r}",
                    suggestion='world = World("my_world")',
                    line=statement.lineno,
                )
            if world_id is not None:
                raise WorldBuilderError(
                    f"this file already declares World({world_id!r}); an intent file "
                    "describes exactly one world",
                    suggestion=f"remove this line and keep World({world_id!r})",
                    line=statement.lineno,
                )
            world_id = identifier
            bindings[statement.targets[0].id] = {"kind": "world"}
            continue
        if isinstance(statement, ast.Expr) and isinstance(statement.value, ast.Call):
            call = statement.value
            if (
                not isinstance(call.func, ast.Attribute)
                or not isinstance(call.func.value, ast.Name)
                or bindings.get(call.func.value.id, {}).get("kind") != "world"
                or call.func.attr != "landform"
            ):
                bound = sorted(bindings) or ["world"]
                raise WorldBuilderError(
                    f"the only statement allowed here is "
                    f"{bound[0]}.landform(...); nothing else runs",
                    suggestion=(
                        f"{bound[0]}.landform(\"west_ridge\", "
                        f"composition={_COMPOSITION_TEMPLATES['ridge_network']})"
                    ),
                    line=statement.lineno,
                )
            owner = call.func.value.id
            if len(call.args) != 1:
                raise WorldBuilderError(
                    f"{owner}.landform takes exactly one positional argument, the id "
                    f"of the feature to shape, then composition=... by keyword",
                    suggestion=(
                        f"{owner}.landform(\"west_ridge\", "
                        f"composition={_COMPOSITION_TEMPLATES['ridge_network']})"
                    ),
                    line=statement.lineno,
                )
            feature_id = _literal(call.args[0], repairs)
            if not isinstance(feature_id, str) or not feature_id:
                raise WorldBuilderError(
                    f"a landform id must be a non-empty string, not {feature_id!r}",
                    suggestion=f'{owner}.landform("west_ridge", composition=...)',
                    line=statement.lineno,
                )
            values = _keywords(call, repairs)
            if set(values) != {"composition"} or not isinstance(
                values["composition"], dict
            ):
                extra = sorted(set(values) - {"composition"})
                detail = (
                    f"got unexpected keyword(s): {', '.join(extra)}"
                    if extra
                    else "composition=... is required"
                )
                raise WorldBuilderError(
                    f"{owner}.landform currently accepts only composition=...; {detail}",
                    suggestion=(
                        f"{owner}.landform({feature_id!r}, "
                        f"composition={_COMPOSITION_TEMPLATES['ridge_network']})"
                    ),
                    line=statement.lineno,
                )
            patches.append(
                {
                    "feature_id": feature_id,
                    "generation": {"composition": values["composition"]},
                    "source": {"path": source_name, "line": statement.lineno},
                }
            )
            continue
        raise WorldBuilderError(
            f"a {type(statement).__name__} statement cannot appear in an intent file. "
            "An intent file describes a world; it does not compute one, so there are "
            "no loops, conditionals, functions, or imports beyond 'worldbuilder'",
            suggestion=(
                "write one world.landform(...) line per feature you want to shape"
            ),
            line=getattr(statement, "lineno", None),
        )
    if world_id is None:
        raise WorldBuilderError(
            "this file never declares a World, so there is nothing to patch",
            suggestion='world = World("my_world")',
        )
    if not patches:
        raise WorldBuilderError(
            f"World({world_id!r}) is declared but no landform is shaped, so the file "
            "would change nothing",
            suggestion=(
                f"world.landform(\"west_ridge\", "
                f"composition={_COMPOSITION_TEMPLATES['ridge_network']})"
            ),
        )
    identifiers = [patch["feature_id"] for patch in patches]
    if len(identifiers) != len(set(identifiers)):
        repeated = sorted({name for name in identifiers if identifiers.count(name) > 1})
        raise WorldBuilderError(
            f"each feature may be shaped only once, but {', '.join(repr(n) for n in repeated)} "
            "appears more than once; the later call would silently win",
            suggestion=f"keep one world.landform({repeated[0]!r}, ...) call and delete the rest",
        )
    intent: dict[str, Any] = {
        "schema_version": INTENT_VERSION,
        "world_id": world_id,
        "source_name": source_name,
        "patches": patches,
    }
    if repairs:
        intent["repairs"] = repairs
    return intent


def apply_intent(zone_spec: dict[str, Any], intent: dict[str, Any]) -> dict[str, Any]:
    if intent.get("schema_version") != INTENT_VERSION:
        raise WorldBuilderError(
            f"this intent is {intent.get('schema_version')!r}, but this compiler "
            f"reads {INTENT_VERSION}"
        )
    result = copy.deepcopy(zone_spec)
    zone_id = result.get("zone", {}).get("id")
    if intent.get("world_id") != zone_id:
        raise WorldBuilderError(
            f"this intent describes World({intent.get('world_id')!r}) but the zone "
            f"being built is {zone_id!r}",
            suggestion=f'world = World({zone_id!r})',
        )
    by_id = {
        feature.get("id"): feature
        for feature in result.get("features", [])
        if isinstance(feature, dict)
    }
    landforms = {
        name: feature
        for name, feature in by_id.items()
        if name and feature.get("category") == "landform"
    }
    for patch in intent.get("patches", []):
        feature_id = patch.get("feature_id")
        feature = by_id.get(feature_id)
        if feature is None:
            near = _nearest(feature_id, landforms)
            if near is not None:
                suggestion = _landform_call(near, landforms[near])
                detail = f"; did you mean {near!r}?"
            else:
                suggestion = None
                detail = (
                    f". The landforms in this world are: {_one_of(landforms)}"
                    if landforms
                    else ". This world has no landforms to shape."
                )
            raise WorldBuilderError(
                f"there is no feature called {feature_id!r} in this world{detail}",
                suggestion=suggestion,
            )
        if feature.get("category") != "landform":
            raise WorldBuilderError(
                f"{feature_id!r} is a {feature.get('category')!r} feature, not a "
                f"landform, so it has no terrain composition. Shapeable landforms "
                f"are: {_one_of(landforms)}",
                suggestion=(
                    _landform_call(*next(iter(landforms.items()))) if landforms else None
                ),
            )
        composition = patch["generation"]["composition"]
        semantic = str(feature.get("semantic"))
        allowed = _SEMANTIC_COMPOSITIONS.get(semantic, set())
        if composition["pattern"] not in allowed:
            if not allowed:
                raise WorldBuilderError(
                    f"{feature_id!r} is a {semantic!r} landform, which has no "
                    f"authorable composition patterns yet; leave it to the compiler",
                    suggestion=f"delete the world.landform({feature_id!r}, ...) call",
                )
            raise WorldBuilderError(
                f"{feature_id!r} is a {semantic!r} landform, which is built from "
                f"{' or '.join(sorted(allowed))}, not "
                f"{composition['pattern']!r}",
                suggestion=_landform_call(feature_id, feature),
            )
        feature.setdefault("generation", {})["composition"] = composition
        feature["generation"]["intent_source"] = patch["source"]
    result["world_intent"] = {
        "schema_version": INTENT_VERSION,
        "source_name": intent.get("source_name"),
        "patch_count": len(intent.get("patches", [])),
    }
    return result


_SCAFFOLD_HEADER = """\
# WorldBuilder intent for {zone_id}
#
# This file describes a world; it does not compute one. There are no loops,
# conditionals, functions, or imports beyond 'worldbuilder'. Edit the values
# below and recompile -- every landform in this world is already listed.
#
# Each landform accepts only the composition its terrain semantic allows:
{semantic_rules}
#
# spines        whole number 1-8, how many ridge spines the landform is built from
# elevation_bias   0.0-1.0, higher lifts the landform's mass upward
# along_jitter     0.0-0.5, waviness along each spine
# cross_jitter     0.0-0.5, waviness across each spine
#
# along_jitter and cross_jitter above are DOCUMENTED limits, not BUILDABLE
# ones: any value in 0.0-0.5 parses and compiles, but a value that roughens
# the ridge flanks too much can still fail the terrain accessibility gate at
# build time. That failure names which landform and knob to lower -- see
# build_zone.py's accessibility diagnosis -- so treat a rejection there as
# the real bound, not this comment.
# silhouette    {silhouettes}
# massing       {massing}
# surface       {surfaces}
# dressing      {dressing}
"""


def _format_scalar(value: Any) -> str:
    """Render a scalar so re-parsing it yields the identical float."""
    return repr(float(value))


def _scaffold_landform(
    feature_id: str, feature: dict[str, Any], world_var: str
) -> str:
    semantic = str(feature.get("semantic"))
    allowed = _SEMANTIC_COMPOSITIONS[semantic]
    existing = feature.get("generation", {}).get("composition") or {}
    pattern = existing.get("pattern")
    if pattern not in allowed:
        pattern = sorted(allowed)[0]

    scalars = dict(_PATTERN_SCALARS[pattern])
    spines = existing.get("spine_count", scalars["spines"])
    lines = [
        f"# {feature_id} -- {semantic} (accepts: {' or '.join(sorted(allowed))})",
        f"{world_var}.landform(",
        f"    {feature_id!r},",
        f"    composition={pattern}(",
        f"        spines={int(spines)},",
    ]
    for key in ("elevation_bias", "along_jitter", "cross_jitter"):
        value = existing.get(key, scalars[key])
        lines.append(f"        {key}={_format_scalar(value)},")
    for key in ("silhouette", "massing", "surface", "dressing"):
        value = existing.get(key, _PATTERN_DEFAULTS[pattern][key])
        lines.append(f"        {key}={str(value)!r},")
    lines.extend(["    ),", ")"])
    return "\n".join(lines)


def scaffold_intent(zone_spec: dict[str, Any], *, world_var: str = "world") -> str:
    """Emit a complete, valid intent file for every shapeable landform in a zone.

    The blank page is where weak models fail, so no model should ever face one.
    The result compiles and applies as-is, reproducing the world it came from;
    authoring is then editing named values rather than recalling a vocabulary.
    """
    zone_id = zone_spec.get("zone", {}).get("id")
    if not isinstance(zone_id, str) or not zone_id:
        raise WorldBuilderError(
            "this zone spec has no zone.id, so no intent file can be written for it"
        )
    features = [
        feature
        for feature in zone_spec.get("features", [])
        if isinstance(feature, dict)
        and feature.get("category") == "landform"
        and isinstance(feature.get("id"), str)
    ]
    shapeable = [
        feature
        for feature in features
        if str(feature.get("semantic")) in _SEMANTIC_COMPOSITIONS
    ]
    if not shapeable:
        known = _one_of(_SEMANTIC_COMPOSITIONS)
        raise WorldBuilderError(
            f"{zone_id!r} has no landform whose composition can be authored yet; "
            f"authorable terrain semantics are: {known}"
        )

    patterns = sorted(
        {
            (
                feature.get("generation", {}).get("composition", {}).get("pattern")
                if feature.get("generation", {}).get("composition", {}).get("pattern")
                in _SEMANTIC_COMPOSITIONS[str(feature.get("semantic"))]
                else sorted(_SEMANTIC_COMPOSITIONS[str(feature.get("semantic"))])[0]
            )
            for feature in shapeable
        }
    )
    semantic_rules = "\n".join(
        f"#   {semantic:<16} {' or '.join(sorted(allowed))}"
        for semantic, allowed in sorted(_SEMANTIC_COMPOSITIONS.items())
    )
    header = _SCAFFOLD_HEADER.format(
        zone_id=zone_id,
        semantic_rules=semantic_rules,
        silhouettes=_one_of(_SILHOUETTES),
        massing=_one_of(_MASSING),
        surfaces=_one_of(_SURFACES),
        dressing=_one_of(_DRESSING),
    )
    body = [
        header,
        f"from worldbuilder import World, {', '.join(patterns)}",
        "",
        f"{world_var} = World({zone_id!r})",
        "",
    ]
    body.extend(
        _scaffold_landform(str(feature["id"]), feature, world_var)
        for feature in shapeable
    )
    skipped = [
        str(feature["id"]) for feature in features if feature not in shapeable
    ]
    if skipped:
        body.append(
            "# Left to the compiler (no authorable composition yet): "
            + ", ".join(sorted(skipped))
        )
    return "\n".join(body) + "\n"


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Compile sandboxed Python-shaped WorldBuilder intent"
    )
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--scaffold",
        action="store_true",
        help=(
            "treat SOURCE as a zone_spec.json and write a complete, ready-to-edit "
            "intent file listing every shapeable landform, instead of compiling"
        ),
    )
    parser.add_argument(
        "--lenient",
        action="store_true",
        help=(
            "repair recoverable near-misses (out-of-range values, misspelled "
            "enum values) and report them instead of failing; authoring aid only, "
            "certification stays strict"
        ),
    )
    args = parser.parse_args(argv)
    if args.scaffold:
        try:
            zone_spec = json.loads(args.source.read_text(encoding="utf-8"))
            source = scaffold_intent(zone_spec)
        except (OSError, ValueError) as exc:
            print(f"{args.source}: {exc}", file=sys.stderr)
            return 2
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(source, encoding="utf-8")
        landforms = source.count(".landform(")
        print(f"Scaffolded {landforms} landforms into {args.output}")
        return 0
    try:
        intent = compile_intent(
            args.source.read_text(encoding="utf-8"),
            lenient=args.lenient,
            source_name=str(args.source)
        )
    except (OSError, WorldBuilderError) as exc:
        # Printed rather than routed through parser.error so a multi-line
        # repair suggestion survives intact instead of trailing a usage dump.
        print(f"{args.source}: {exc}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(intent, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for repair in intent.get("repairs", []):
        print(f"repaired: {repair}", file=sys.stderr)
    print(f"Compiled {len(intent['patches'])} WorldBuilder intent patches")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
