"""Separable solid parts of a composite asset, from its glTF header.

A settlement cluster is not a building -- it is five houses, a well, a terrace
and a watchtower, with streets between them. `collision_plan` gave the whole
asset one box, so every village was a solid slab 21 m across, and the three
lanes that run through the villages were blocked by geometry a player can
plainly see they should walk between (see D13).

The mesh already knows better. glTF stores per-accessor `min`/`max` on every
POSITION attribute, so exact local bounds for every node are available from the
JSON chunk alone -- no vertex data is read and no Blender is needed.

**Parts are grouped, not emitted one-per-node.** A house is a foundation, walls,
a roof, a door, two windows and a chimney: seven nodes that a body collides with
as one object. Emitting seven colliders per house would be seven times the
physics cost for the same shape, and a window frame is not a thing you walk
into separately from its wall. Nodes are grouped by name prefix, which is what
the kit generators already encode (`House_00_*`, `Watchtower_*`).

**Whether a part blocks movement is not decided here.** That depends on the
agent's climb height and the terrain under the part, neither of which this
module can see. It reports geometry; `navigation_plan` decides passability.
"""

from __future__ import annotations

import json
import re
import struct
from pathlib import Path
from typing import Any

# Whether a part obstructs is deliberately NOT decided here, and not by name.
# A keep's `Bedrock_plinth` is 264x7x264 in asset units -- a 34 m slab 0.9 m
# tall -- and its `Inner_courtyard` is 0.26 m tall. Those are floors, and a
# name list that happened to contain "plinth" and "courtyard" would be a guess
# dressed as a rule, wrong for the next kit.
#
# The physical answer needs the agent and the terrain, which only `navigation_plan`
# has: a body steps onto anything shorter than its climb height. So parts are
# emitted with honest geometry and the movement question is answered where it
# can be answered correctly.

# Trailing part-name segments stripped to find the group a node belongs to.
# `House_00_slate_roof` and `House_00_door` are both `House_00`.
# `thatch_roof`, `chimney_<n>` and `timber_<n>` were added for
# `generate_highland_building_kit.py`'s individual cottage/well/watchtower
# assets: multi-chimney cottages and the well's paired timber posts need a
# numbered suffix the same way `window` and `merlon` already had one, and
# thatch roofs need the same two-word treatment `slate_roof` already gets --
# see that pattern's own alternative for why a bare `roof` alternative is not
# enough on its own.
_GROUP_PATTERN = re.compile(
    r"^(?P<group>.*?)(?:_(?:chimney(?:_\d+)?|door|foundation|plaster|"
    r"slate_roof|thatch_roof|roof|window(?:_-?\d+)?|wall|stone|"
    r"merlon(?:_\d+)?|timber(?:_-?\d+)?))?$",
    re.IGNORECASE,
)


class AssetPartsError(ValueError):
    """The asset's parts cannot be read."""


def _group_of(name: str) -> str:
    """The object a node belongs to.

    `House_00_slate_roof` -> `House_00`; `Watchtower_merlon_03` -> `Watchtower`.
    A node whose name matches no known part suffix is its own group, so an
    unfamiliar kit degrades to one collider per node rather than to nothing.
    """
    match = _GROUP_PATTERN.match(name or "")
    group = (match.group("group") if match else name) or name
    return group.rstrip("_") or name


def _node_transform(node: dict[str, Any]) -> tuple[list[float], list[float]]:
    """Translation and scale of a node. Rotation is deliberately not applied.

    The kit generators emit axis-aligned parts, and a bounds-of-rotated-bounds
    would inflate rather than tighten. If a kit ever rotates its parts this
    should grow a real transform -- it would be visible as colliders larger
    than their geometry, not as silent misplacement.
    """
    translation = [float(v) for v in node.get("translation", (0.0, 0.0, 0.0))]
    scale = [float(v) for v in node.get("scale", (1.0, 1.0, 1.0))]
    return translation, scale


def read_gltf(path: Path) -> dict[str, Any]:
    """The JSON chunk of a .glb, without touching the binary payload."""
    data = path.read_bytes()
    if len(data) < 20 or data[:4] != b"glTF":
        raise AssetPartsError("%s is not a binary glTF" % path)
    length = struct.unpack("<I", data[12:16])[0]
    try:
        return json.loads(data[20 : 20 + length])
    except ValueError as exc:
        raise AssetPartsError("%s has an unreadable glTF header: %s" % (path, exc))


def parts(path: Path) -> list[dict[str, Any]]:
    """Local-space bounds of each separable solid part.

    Bounds are in the asset's own units, matching `asset_visual_preflight`'s
    `bounds_m`, so a caller scales and places them exactly as it already does
    for the whole-asset box.
    """
    document = read_gltf(path)
    accessors = document.get("accessors", [])
    meshes = document.get("meshes", [])

    grouped: dict[str, dict[str, Any]] = {}
    for node in document.get("nodes", []):
        index = node.get("mesh")
        if index is None or index >= len(meshes):
            continue
        translation, scale = _node_transform(node)
        for primitive in meshes[index].get("primitives", []):
            position = primitive.get("attributes", {}).get("POSITION")
            if position is None or position >= len(accessors):
                continue
            accessor = accessors[position]
            low, high = accessor.get("min"), accessor.get("max")
            if not low or not high:
                # Without bounds we cannot place a collider, and guessing one
                # is worse than saying the asset is not separable.
                raise AssetPartsError(
                    "%s: node %r has no POSITION bounds" % (path, node.get("name"))
                )
            minimum = [float(low[i]) * scale[i] + translation[i] for i in range(3)]
            maximum = [float(high[i]) * scale[i] + translation[i] for i in range(3)]
            # Negative scale flips the interval; a min above its max would
            # produce a collider with negative extent.
            minimum, maximum = (
                [min(minimum[i], maximum[i]) for i in range(3)],
                [max(minimum[i], maximum[i]) for i in range(3)],
            )
            group = _group_of(node.get("name", ""))
            entry = grouped.setdefault(group, {"name": group, "min": minimum, "max": maximum})
            entry["min"] = [min(entry["min"][i], minimum[i]) for i in range(3)]
            entry["max"] = [max(entry["max"][i], maximum[i]) for i in range(3)]

    result = []
    for entry in grouped.values():
        entry["size"] = [entry["max"][i] - entry["min"][i] for i in range(3)]
        result.append(entry)
    result.sort(key=lambda part: part["name"])
    return result


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("asset", type=Path)
    arguments = parser.parse_args(argv)

    for part in parts(arguments.asset):
        print(
            "%-28s size %6.1f x %6.1f x %6.1f  at %7.1f, %7.1f"
            % (
                part["name"],
                part["size"][0],
                part["size"][1],
                part["size"][2],
                (part["min"][0] + part["max"][0]) * 0.5,
                (part["min"][2] + part["max"][2]) * 0.5,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
