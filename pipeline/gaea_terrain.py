#!/usr/bin/env python3
"""Read and edit Gaea `.terrain` graphs without opening Gaea.

`.terrain` is plain JSON -- a Newtonsoft-serialised object graph using `$id` /
`$ref` reference tracking -- which is what makes WGE able to drive Gaea as an
adapter target rather than a plugin host (open decisions item 9).

**Why this exists at all.** A Gaea graph only produces files for nodes that
carry a `SaveDefinition`. Only 3 of the 59 bundled examples have one, and a
graph without one builds successfully and writes nothing, which looks exactly
like a failure. More to the point, the examples that *do* export save colour
renders (`Cartography`, `Shade`), and WGE needs the **heightfield**.

A `SaveDefinition` is small and entirely writable:

    {"$id": "89", "Node": 720, "Filename": "Cartography",
     "Format": "PNG16", "IsEnabled": true,
     "DisabledInProfiles": {"$id": "90", "$values": []}}

so adding a height export is an edit, not an authoring session. That matters
because the Gaea 3D viewport is unreliable under Proton, and needing the GUI to
mark an export would put a flaky interactive step in an automated pipeline.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterator

# 16-bit greyscale PNG. Chosen over 8-bit because a 256-step heightfield
# terraces visibly once it is stretched over real relief, and over EXR/R32
# because PIL reads PNG16 without an extra dependency in the import path.
HEIGHT_FORMAT = "PNG16"


def _strip_trailing_commas(text: str) -> str:
    """Drop commas that directly precede `}` or `]`, outside string literals.

    Gaea writes through Newtonsoft, which accepts trailing commas; Python's
    `json` does not. One of the 59 bundled examples (`Glacier - Complex Setup`)
    has one, so a strict loader cannot open every graph Gaea can.
    """
    out: list[str] = []
    in_string = False
    escaped = False
    pending_comma: list[str] = []  # a comma plus any whitespace after it
    for char in text:
        if in_string:
            out.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if pending_comma:
            if char.isspace():
                pending_comma.append(char)
                continue
            if char in "}]":
                out.extend(pending_comma[1:])  # keep the whitespace, drop the comma
            else:
                out.extend(pending_comma)
            pending_comma = []
        if char == ",":
            pending_comma = [char]
            continue
        if char == '"':
            in_string = True
        out.append(char)
    out.extend(pending_comma)
    return "".join(out)


def load_graph(path: Path) -> dict:
    """Read a `.terrain` graph, accepting the non-strict JSON Gaea itself accepts."""
    text = Path(path).read_text(encoding="utf-8-sig")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return json.loads(_strip_trailing_commas(text))


def walk(node: Any) -> Iterator[dict]:
    """Every dict in the object graph, depth first."""
    if isinstance(node, dict):
        yield node
        for value in node.values():
            yield from walk(value)
    elif isinstance(node, list):
        for value in node:
            yield from walk(value)


def nodes(graph: dict) -> list[dict]:
    """The graph's actual terrain nodes, deduplicated by `Id`.

    Identified structurally -- `Id` + `Name` + `Position` together -- rather
    than by `$type`, because the type strings carry assembly-qualified names
    that differ between Gaea builds.
    """
    seen: set[int] = set()
    found: list[dict] = []
    for candidate in walk(graph):
        identifier = candidate.get("Id")
        if not isinstance(identifier, int):
            continue
        if "Name" not in candidate or "Position" not in candidate:
            continue
        if identifier in seen:
            continue
        seen.add(identifier)
        found.append(candidate)
    return found


def next_reference_id(graph: dict) -> int:
    """One past the highest `$id`, so an inserted object cannot collide.

    Newtonsoft resolves `$ref` by exact string match against `$id`, so a
    duplicate would silently alias two different objects.
    """
    highest = 0
    for candidate in walk(graph):
        value = candidate.get("$id")
        if isinstance(value, str) and value.isdigit():
            highest = max(highest, int(value))
    return highest + 1


def build_definitions(graph: dict) -> list[dict]:
    """Every `BuildDefinition` block in the graph (one per asset in practice)."""
    return [
        candidate["BuildDefinition"]
        for candidate in walk(graph)
        if isinstance(candidate.get("BuildDefinition"), dict)
    ]


def ensure_build_type(graph: dict) -> int:
    """Give every `BuildDefinition` a `Type`, defaulting to `Standard`.

    Measured 2026-10-05: Swarm exits 0 in about 4 s and writes nothing for a
    graph whose `BuildDefinition` has no `Type`, exactly as it does for a graph
    with no `SaveDefinition`. 29 of the 59 bundled examples have no `Type`
    (the GUI supplies it when a user opens the build dialog). Adding
    `"Type": "Standard"` to `Structure - Custom Mountain Range` made it build.
    Returns how many definitions were filled in.
    """
    filled = 0
    for definition in build_definitions(graph):
        if "Type" not in definition:
            # Keep `$id` first, as Newtonsoft writes it; it is a reference
            # anchor and its position is cosmetic, but a diff stays readable.
            items = list(definition.items())
            definition.clear()
            definition.update(items[:1])
            definition["Type"] = "Standard"
            definition.update(items[1:])
            filled += 1
    return filled


def add_save_definition(
    graph: dict, node_id: int, filename: str, fmt: str = HEIGHT_FORMAT
) -> dict:
    """Mark `node_id` for export. Replaces any existing definition on it.

    Also fills in a missing build `Type` (`ensure_build_type`), because a graph
    marked for export without one still builds nothing.
    """
    ensure_build_type(graph)
    for candidate in nodes(graph):
        if candidate.get("Id") != node_id:
            continue
        reference = next_reference_id(graph)
        candidate["SaveDefinition"] = {
            "$id": str(reference),
            "Node": node_id,
            "Filename": filename,
            "Format": fmt,
            "IsEnabled": True,
            "DisabledInProfiles": {"$id": str(reference + 1), "$values": []},
        }
        return candidate
    raise SystemExit(f"no node with Id {node_id} in this graph")


def node_container(graph: dict) -> dict:
    """The `Nodes` map, keyed by stringified node `Id`.

    Found structurally rather than by path, because the path
    (`/Assets/$values[0]/Terrain/Nodes`) is an implementation detail of the file
    format and the assembly-qualified `$type` strings already differ between
    Gaea builds.
    """
    for candidate in walk(graph):
        keys = [key for key in candidate if key.isdigit()]
        if keys and all(
            isinstance(candidate[key], dict) and candidate[key].get("Id") == int(key)
            for key in keys
        ):
            return candidate
    raise SystemExit("this graph has no recognisable node collection")


# Ports a node exposes, beyond the primary in/out. Erosion2 publishes its
# intermediate fields as extra outputs, and omitting them makes the node load
# without the outputs a later graph edit would want to wire.
EXTRA_OUTPUTS = {
    "Erosion2": ("Flow", "Wear", "Deposits"),
    "Rivers": ("Rivers", "Depth", "Surface", "Direction"),
}

# Starting values for an inserted node. Taken from the bundled
# `Cartography - 3D Map`, which runs `MountainRange -> Combine -> Erosion2` --
# a graph that demonstrably produces good landform, so it is a better default
# than numbers picked here.
NODE_DEFAULTS = {
    "Erosion2": {
        "Duration": 116.36197,
        "Downcutting": 0.24590053,
        "ErosionScale": 890.32446,
        "Seed": 5466,
        "CoarseSedimentsDischargeAmount": 1.0,
        "CoarseSedimentsDischargeAngle": 13.740435,
    },
    "Rivers": {"Water": 0.35, "Width": 0.4, "Depth": 0.35, "Downcutting": 0.15, "Seed": 1337},
}


def insert_after(graph: dict, source_id: int, node_type: str) -> dict:
    """Splice a new node between `source_id` and everything it feeds.

    Connections live on the *receiving* node's input port as a `Record`
    (`From` / `To` / `FromPort` / `ToPort`), so inserting is two edits: give the
    new node an input reading from `source_id`, then repoint every consumer that
    was reading `source_id` at the new node instead.
    """
    container = node_container(graph)
    source = container.get(str(source_id))
    if source is None:
        raise SystemExit(f"no node with Id {source_id} in this graph")

    taken = {int(key) for key in container if key.isdigit()}
    new_id = max(taken) + 1
    reference = next_reference_id(graph)

    def port(name: str, kind: str, record: dict | None = None) -> dict:
        nonlocal reference
        reference += 1
        entry: dict = {"$id": str(reference), "Name": name, "Type": kind}
        if record is not None:
            reference += 1
            entry["Record"] = {"$id": str(reference), **record}
        entry["IsExporting"] = True
        entry["Parent"] = {"$ref": str(reference_root)}
        return entry

    reference_root = reference
    position = source.get("Position", {})
    node = {
        "$id": str(reference_root),
        "$type": f"QuadSpinner.Gaea.Nodes.{node_type}, Gaea.Nodes",
        **NODE_DEFAULTS.get(node_type, {}),
        "Id": new_id,
        "Name": node_type,
        "NodeSize": "Small",
        "Position": {
            "$id": str(reference_root + 1000),
            # Offset so the inserted node does not land underneath its source in
            # the GUI, which reads as a missing node.
            "X": float(position.get("X", 26000.0)) + 115.0,
            "Y": float(position.get("Y", 26000.0)) + 130.0,
        },
    }
    reference += 1
    ports = [port("In", "PrimaryIn, Required", {
        "From": source_id, "To": new_id, "FromPort": "Out", "ToPort": "In", "IsValid": True,
    })]
    ports.append(port("Out", "PrimaryOut"))
    for extra in EXTRA_OUTPUTS.get(node_type, ()):
        ports.append(port(extra, "Out"))
    reference += 1
    node["Ports"] = {"$id": str(reference), "$values": ports}
    reference += 1
    node["Modifiers"] = {"$id": str(reference), "$values": []}

    # Repoint the old consumers. Done after the new node is built so a failure
    # part-way cannot leave the graph wired to a node that does not exist.
    moved = 0
    for candidate in container.values():
        if not isinstance(candidate, dict) or candidate.get("Id") == new_id:
            continue
        for entry in candidate.get("Ports", {}).get("$values", []):
            record = entry.get("Record")
            if record and record.get("From") == source_id:
                record["From"] = new_id
                moved += 1

    container[str(new_id)] = node
    node["_rewired"] = moved
    return node


class _References:
    """Hands out unique `$id` strings for objects added to one graph."""

    def __init__(self, start: int) -> None:
        self.next = start

    def take(self) -> str:
        self.next += 1
        return str(self.next)


def _make_node(
    references: _References,
    node_type: str,
    node_id: int,
    position: tuple[float, float],
    wired_inputs: dict[str, int],
    spare_inputs: tuple[str, ...] = (),
) -> dict:
    """A node with its ports, wired to the sources named in `wired_inputs`.

    `wired_inputs` maps this node's port name to the source node `Id`. The
    connection lives on *this* node's port, which is why wiring an input never
    touches the upstream node.
    """
    root = references.take()
    ports: list[dict] = []

    def add_port(name: str, kind: str, source: int | None) -> None:
        entry: dict = {"$id": references.take(), "Name": name, "Type": kind}
        if source is not None:
            entry["Record"] = {
                "$id": references.take(),
                "From": source,
                "To": node_id,
                "FromPort": "Out",
                "ToPort": name,
                "IsValid": True,
            }
        entry["IsExporting"] = True
        entry["Parent"] = {"$ref": root}
        ports.append(entry)

    add_port("In", "PrimaryIn, Required" if "In" in wired_inputs else "PrimaryIn",
             wired_inputs.get("In"))
    add_port("Out", "PrimaryOut", None)
    for name in spare_inputs:
        add_port(name, "In", wired_inputs.get(name))
    for name in EXTRA_OUTPUTS.get(node_type, ()):
        add_port(name, "Out", None)

    return {
        "$id": root,
        "$type": f"QuadSpinner.Gaea.Nodes.{node_type}, Gaea.Nodes",
        **NODE_DEFAULTS.get(node_type, {}),
        "Id": node_id,
        "Name": node_type,
        "NodeSize": "Small",
        "Position": {"$id": references.take(), "X": position[0], "Y": position[1]},
        "Ports": {"$id": references.take(), "$values": ports},
        "Modifiers": {"$id": references.take(), "$values": []},
    }


def _repoint_consumers(container: dict, old_source: int, new_source: int, exclude: set[int]) -> int:
    """Make everything that read `old_source` read `new_source` instead."""
    moved = 0
    for candidate in container.values():
        if not isinstance(candidate, dict) or candidate.get("Id") in exclude:
            continue
        for entry in candidate.get("Ports", {}).get("$values", []):
            record = entry.get("Record")
            if record and record.get("From") == old_source:
                record["From"] = new_source
                moved += 1
    return moved


def insert_basin(graph: dict, source_id: int, ratio: float = 0.5) -> dict:
    """Add a radial basin to `source_id`'s output, before whatever consumes it.

    Builds `RadialGradient -> Combine(Subtract)` and splices the Combine in, so the
    terrain gains a bowl: low in the middle, high around the rim.

    **Placement is the whole point.** The basin goes *upstream of erosion*, so
    the rim is carved by drainage rather than being the final word on amplitude.
    That is precisely the condition docs/integrations/gaea-programme.md 5.5 sets for a basin not
    being the abandoned massif defect in polar form -- the erosion has to be
    strong enough to break the monotonicity the bias imposes, and putting the
    bias before the solver is what gives it the chance.
    """
    container = node_container(graph)
    source = container.get(str(source_id))
    if source is None:
        raise SystemExit(f"no node with Id {source_id} in this graph")

    taken = {int(key) for key in container if key.isdigit()}
    gradient_id, combine_id = max(taken) + 1, max(taken) + 2
    references = _References(next_reference_id(graph))
    position = source.get("Position", {})
    x, y = float(position.get("X", 26000.0)), float(position.get("Y", 26000.0))

    gradient = _make_node(references, "RadialGradient", gradient_id, (x, y + 260.0), {})
    combine = _make_node(
        references,
        "Combine",
        combine_id,
        (x + 115.0, y + 130.0),
        {"In": source_id, "Input2": gradient_id},
        spare_inputs=("Input2", "Mask", "Input3"),
    )
    combine["PortCount"] = 3
    # Subtract, not Add. `RadialGradient` is **centre-high**, so adding it
    # raises the middle into a dome -- measured, the first attempt gave 285 m at
    # the centre against 52 m at the rim. Subtracting pushes the centre down and
    # leaves the rim, which is the bowl.
    combine["Mode"] = "Subtract"
    combine["Ratio"] = float(ratio)

    moved = _repoint_consumers(
        container, source_id, combine_id, exclude={gradient_id, combine_id}
    )
    container[str(gradient_id)] = gradient
    container[str(combine_id)] = combine
    combine["_rewired"] = moved
    return combine


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("terrain", type=Path)
    parser.add_argument("--list", action="store_true", help="list nodes and exit")
    parser.add_argument("--export-node", type=int, help="node Id to mark for export")
    parser.add_argument(
        "--insert-after",
        type=int,
        metavar="ID",
        help="splice a new node between this node and everything it feeds",
    )
    parser.add_argument(
        "--node-type", default="Erosion2", help="node type for --insert-after"
    )
    parser.add_argument(
        "--insert-basin",
        type=int,
        metavar="ID",
        help="add a radial basin to this node's output (RadialGradient + Combine)",
    )
    parser.add_argument(
        "--basin-ratio", type=float, default=0.5, help="basin strength for --insert-basin"
    )
    parser.add_argument("--name", default="Height", help="output filename stem")
    parser.add_argument("--format", default=HEIGHT_FORMAT)
    parser.add_argument("--output", type=Path, help="where to write the edited graph")
    arguments = parser.parse_args()

    graph = load_graph(arguments.terrain)

    if arguments.insert_basin is not None:
        combine = insert_basin(graph, arguments.insert_basin, arguments.basin_ratio)
        destination = arguments.output or arguments.terrain
        rewired = combine.pop("_rewired")
        destination.write_text(json.dumps(graph), encoding="utf-8")
        print(
            "basin added after %d as Combine Id %d (ratio %.2f), rewiring %d consumer(s) -> %s"
            % (arguments.insert_basin, combine["Id"], arguments.basin_ratio, rewired,
               destination.name)
        )
        return 0

    if arguments.insert_after is not None:
        inserted = insert_after(graph, arguments.insert_after, arguments.node_type)
        destination = arguments.output or arguments.terrain
        rewired = inserted.pop("_rewired")
        destination.write_text(json.dumps(graph), encoding="utf-8")
        print(
            "inserted %s as Id %d after %d, rewiring %d consumer(s) -> %s"
            % (
                arguments.node_type,
                inserted["Id"],
                arguments.insert_after,
                rewired,
                destination.name,
            )
        )
        return 0

    if arguments.list or arguments.export_node is None:
        for candidate in nodes(graph):
            save = candidate.get("SaveDefinition") or {}
            marker = (
                "  -> %s.%s" % (save.get("Filename"), save.get("Format"))
                if save
                else ""
            )
            print("%6d  %s%s" % (candidate["Id"], candidate.get("Name"), marker))
        return 0

    marked = add_save_definition(
        graph, arguments.export_node, arguments.name, arguments.format
    )
    destination = arguments.output or arguments.terrain
    destination.write_text(json.dumps(graph), encoding="utf-8")
    print(
        "marked node %d (%s) as %s.%s -> %s"
        % (
            marked["Id"],
            marked.get("Name"),
            arguments.name,
            arguments.format,
            destination.name,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
