#!/usr/bin/env python3
"""MCP server that lets an agent drive Gaea through Luxel's own driver.

Landscape parity plan P1 (`docs/world/landscape-parity-plan.md`).
Community Gaea MCP servers exist; this one is used instead because it routes
through `gaea_terrain.py` and `gaea_build.py`, which carry the Proton invocation
traps and the pixel-digest receipts that a generic server does not.

Transport is MCP stdio: newline-delimited JSON-RPC 2.0. It is implemented here
directly rather than through the `mcp` package because the surface needed
(initialize, tools/list, tools/call, ping) is small and the package is not a
dependency of this repo.

Graph edits never write in place. Every edit names an output path, and paths
inside Gaea's bundled `Examples/` folder are refused as outputs, so an agent
cannot damage the reference graphs.
"""

from __future__ import annotations

import json
import sys
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaea_build  # noqa: E402
import gaea_terrain  # noqa: E402

SERVER_INFO = {"name": "luxel-gaea", "version": "1.0.0"}
SUPPORTED_PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")


class ToolError(Exception):
    """A tool-level failure, reported to the agent as `isError` content."""


def _environment() -> gaea_build.GaeaEnvironment:
    return gaea_build.GaeaEnvironment.from_env()


def _examples_dir() -> Path:
    return _environment().gaea_dir / "Examples"


def _require_graph(path: str) -> Path:
    graph = Path(path).expanduser()
    if not graph.is_file():
        raise ToolError(f"no graph at {graph}")
    return graph


def _writable_output(path: str, source: Path) -> Path:
    output = Path(path).expanduser().resolve()
    examples = _examples_dir().resolve()
    if output == source.resolve():
        raise ToolError("output must differ from the input graph; edits never write in place")
    if examples in output.parents:
        raise ToolError(f"refusing to write inside Gaea's bundled examples ({examples})")
    if output.suffix != ".terrain":
        raise ToolError("output must end in .terrain")
    output.parent.mkdir(parents=True, exist_ok=True)
    return output


def _type_name(node: dict) -> str:
    qualified = str(node.get("$type") or node.get("Name"))
    return qualified.split(",")[0].rsplit(".", 1)[-1]


def tool_status(_: dict) -> dict:
    environment = _environment()
    problems = environment.check()
    examples = sorted(p.stem for p in _examples_dir().glob("*.terrain")) if not problems else []
    return {
        "ready": not problems,
        "problems": problems,
        "gaea_dir": str(environment.gaea_dir),
        "gaea": gaea_build.gaea_identity(environment) if not problems else {},
        "example_count": len(examples),
    }


def tool_list_examples(arguments: dict) -> dict:
    needle = str(arguments.get("filter", "")).lower()
    examples = sorted(_examples_dir().glob("*.terrain"))
    return {
        "examples": [
            {"name": p.stem, "path": str(p)}
            for p in examples
            if needle in p.stem.lower()
        ]
    }


def tool_inspect_graph(arguments: dict) -> dict:
    graph_path = _require_graph(arguments["path"])
    graph = gaea_terrain.load_graph(graph_path)
    rows = []
    for node in gaea_terrain.nodes(graph):
        inputs = []
        outputs = []
        for port in node.get("Ports", {}).get("$values", []):
            record = port.get("Record")
            if record:
                inputs.append(
                    {"port": port.get("Name"), "from": record.get("From"), "from_port": record.get("FromPort")}
                )
            elif "Out" in str(port.get("Type", "")):
                outputs.append(port.get("Name"))
        save = node.get("SaveDefinition")
        rows.append(
            {
                "id": node["Id"],
                "type": _type_name(node),
                "name": node.get("Name"),
                "inputs": inputs,
                "outputs": outputs,
                "export": {"filename": save.get("Filename"), "format": save.get("Format")} if save else None,
            }
        )
    consumed = {i["from"] for row in rows for i in row["inputs"]}
    variables = {}
    for candidate in gaea_terrain.walk(graph):
        block = candidate.get("Variables")
        if isinstance(block, dict):
            variables.update({k: v for k, v in block.items() if not k.startswith("$")})
    return {
        "path": str(graph_path),
        "nodes": rows,
        "terminal_nodes": [row["id"] for row in rows if row["id"] not in consumed],
        "exports": [row["id"] for row in rows if row["export"]],
        "variables": variables,
        "buildable": any(row["export"] for row in rows),
    }


def tool_mark_export(arguments: dict) -> dict:
    source = _require_graph(arguments["path"])
    output = _writable_output(arguments["output_path"], source)
    graph = gaea_terrain.load_graph(source)
    try:
        marked = gaea_terrain.add_save_definition(
            graph,
            int(arguments["node_id"]),
            str(arguments.get("name", "Height")),
            str(arguments.get("format", gaea_terrain.HEIGHT_FORMAT)),
        )
    except SystemExit as error:  # the CLI module reports misuse this way
        raise ToolError(str(error)) from None
    output.write_text(json.dumps(graph), encoding="utf-8")
    return {"output_path": str(output), "node_id": marked["Id"], "type": _type_name(marked)}


def tool_insert_node(arguments: dict) -> dict:
    source = _require_graph(arguments["path"])
    output = _writable_output(arguments["output_path"], source)
    graph = gaea_terrain.load_graph(source)
    try:
        inserted = gaea_terrain.insert_after(
            graph, int(arguments["after_node_id"]), str(arguments.get("node_type", "Erosion2"))
        )
    except SystemExit as error:
        raise ToolError(str(error)) from None
    rewired = inserted.pop("_rewired")
    output.write_text(json.dumps(graph), encoding="utf-8")
    return {"output_path": str(output), "node_id": inserted["Id"], "rewired_consumers": rewired}


def tool_build(arguments: dict) -> dict:
    graph = _require_graph(arguments["graph"])
    request = gaea_build.BuildRequest(
        graph=graph,
        output_dir=Path(arguments["output_dir"]).expanduser(),
        resolution=int(arguments.get("resolution", 1024)),
        seed=arguments.get("seed"),
        variables={str(k): str(v) for k, v in (arguments.get("variables") or {}).items()},
        ignore_cache=not bool(arguments.get("use_cache", False)),
        timeout_s=float(arguments.get("timeout_s", 1800)),
    )
    try:
        return gaea_build.build(request)
    except gaea_build.GaeaBuildError as error:
        raise ToolError(str(error)) from None


def tool_compare(arguments: dict) -> dict:
    return gaea_build.compare(Path(arguments["first"]).expanduser(), Path(arguments["second"]).expanduser())


def _schema(properties: dict, required: list[str]) -> dict:
    return {"type": "object", "properties": properties, "required": required, "additionalProperties": False}


_PATH = {"type": "string", "description": "absolute path to a .terrain graph"}

TOOLS = {
    "gaea_status": (
        tool_status,
        "Check that Gaea, Proton and the Swarm prefix are installed and report the Gaea build identity.",
        _schema({}, []),
    ),
    "gaea_list_examples": (
        tool_list_examples,
        "List Gaea's bundled example graphs, optionally filtered by a substring of the name.",
        _schema({"filter": {"type": "string"}}, []),
    ),
    "gaea_inspect_graph": (
        tool_inspect_graph,
        "List a graph's nodes with type, wiring, outputs and export marks, its terminal nodes, "
        "exposed variables, and whether it can build (a graph builds only if a node is marked for export).",
        _schema({"path": _PATH}, ["path"]),
    ),
    "gaea_mark_export": (
        tool_mark_export,
        "Mark a node for export (adds a SaveDefinition) and write the edited graph to output_path. "
        "Required before a graph can build headlessly. Default format PNG16 height.",
        _schema(
            {
                "path": _PATH,
                "node_id": {"type": "integer"},
                "output_path": {"type": "string", "description": "where to write the edited graph (.terrain)"},
                "name": {"type": "string", "description": "output filename stem, default Height"},
                "format": {"type": "string", "description": "Gaea export format, default PNG16"},
            },
            ["path", "node_id", "output_path"],
        ),
    ),
    "gaea_insert_node": (
        tool_insert_node,
        "Splice a new node (default Erosion2) after a node, rewiring everything it fed, and write the "
        "edited graph to output_path.",
        _schema(
            {
                "path": _PATH,
                "after_node_id": {"type": "integer"},
                "node_type": {"type": "string"},
                "output_path": {"type": "string"},
            },
            ["path", "after_node_id", "output_path"],
        ),
    ),
    "gaea_build": (
        tool_build,
        "Build a graph headlessly through Gaea.Swarm under Proton and return a receipt with pixel "
        "digests of every output. Succeeds only if files were written. A 512 build of a simple "
        "eroded mountain takes about 20-25 s; heavy graphs can take many minutes.",
        _schema(
            {
                "graph": _PATH,
                "output_dir": {"type": "string", "description": "where the outputs and receipt are copied"},
                "resolution": {"type": "integer", "enum": list(gaea_build.RESOLUTIONS)},
                "seed": {"type": "integer"},
                "variables": {"type": "object", "additionalProperties": {"type": ["string", "number"]}},
                "use_cache": {"type": "boolean"},
                "timeout_s": {"type": "number"},
            },
            ["graph", "output_dir"],
        ),
    ),
    "gaea_compare_builds": (
        tool_compare,
        "Pixel-compare two build output directories. Eroded graphs are not byte-reproducible; this "
        "measures by how much two builds differ.",
        _schema({"first": {"type": "string"}, "second": {"type": "string"}}, ["first", "second"]),
    ),
}


def _result(request_id, result: dict) -> dict:
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def _error(request_id, code: int, message: str) -> dict:
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def handle(message: dict) -> dict | None:
    """One JSON-RPC message in, one response out (None for notifications)."""
    method = message.get("method")
    request_id = message.get("id")
    is_notification = "id" not in message

    if method == "initialize":
        requested = (message.get("params") or {}).get("protocolVersion")
        version = requested if requested in SUPPORTED_PROTOCOLS else SUPPORTED_PROTOCOLS[0]
        return _result(
            request_id,
            {"protocolVersion": version, "capabilities": {"tools": {}}, "serverInfo": SERVER_INFO},
        )
    if method == "ping":
        return _result(request_id, {})
    if method == "tools/list":
        return _result(
            request_id,
            {
                "tools": [
                    {"name": name, "description": description, "inputSchema": schema}
                    for name, (_, description, schema) in TOOLS.items()
                ]
            },
        )
    if method == "tools/call":
        params = message.get("params") or {}
        entry = TOOLS.get(params.get("name"))
        if entry is None:
            return _error(request_id, -32602, f"unknown tool {params.get('name')!r}")
        function = entry[0]
        try:
            payload = function(params.get("arguments") or {})
            return _result(
                request_id,
                {"content": [{"type": "text", "text": json.dumps(payload, indent=2)}], "isError": False},
            )
        except (ToolError, KeyError, ValueError, OSError) as error:
            text = f"missing argument {error}" if isinstance(error, KeyError) else str(error)
            return _result(request_id, {"content": [{"type": "text", "text": text}], "isError": True})
        except Exception:  # noqa: BLE001 -- a tool bug must not kill the server
            return _result(
                request_id,
                {"content": [{"type": "text", "text": traceback.format_exc()}], "isError": True},
            )
    if is_notification:
        return None
    return _error(request_id, -32601, f"method not found: {method}")


def serve(stdin=sys.stdin, stdout=sys.stdout) -> None:
    for line in stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            response = _error(None, -32700, f"parse error: {error}")
        else:
            response = handle(message)
        if response is not None:
            stdout.write(json.dumps(response) + "\n")
            stdout.flush()


if __name__ == "__main__":
    serve()
