"""Thin model-facing operation surface for the native WGE control plane.

This module is intentionally transport-only.  It validates the bounded request
shape and resource roots, then delegates semantic work to the Rust control
plane binary.  It does not compute candidate identity, interpret evidence,
move pointers, or decide gate policy.
"""

from __future__ import annotations

import json
import os
import subprocess
import argparse
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[1]
PROTOCOL_SCHEMA = "wge.agent-operation/v1"
MCP_PROTOCOL_VERSION = "2024-11-05"
MCP_SERVER_NAME = "wge-native"
MAX_REQUEST_BYTES = 256 * 1024
MAX_RESPONSE_BYTES = 4 * 1024 * 1024
MAX_TIMEOUT_SECONDS = 120

OPERATIONS = (
    "inspect_project",
    "inspect_current",
    "inspect_artifact",
    "inspect_failures",
    "propose_work",
    "apply_work_order",
    "build_candidate",
    "attach_evidence",
    "run_playtest",
    "capture_evidence",
    "evaluate_candidate",
    "propose_repair",
    "apply_repair",
    "verify_candidate",
    "commit_candidate",
    "rollback",
)


class AgentSurfaceError(RuntimeError):
    """A transport, request-boundary, or delegated native failure."""


@dataclass(frozen=True)
class SurfaceConfig:
    project_root: Path
    control_plane: Path = ROOT / "world_core" / "target" / "debug" / "wge-control-plane"
    timeout_seconds: int = 120
    max_response_bytes: int = MAX_RESPONSE_BYTES

    def validated(self) -> "SurfaceConfig":
        root = self.project_root.expanduser().resolve()
        if not root.is_dir():
            raise AgentSurfaceError(f"project_root is not a directory: {root}")
        timeout = max(1, min(self.timeout_seconds, MAX_TIMEOUT_SECONDS))
        if self.max_response_bytes <= 0 or self.max_response_bytes > MAX_RESPONSE_BYTES:
            raise AgentSurfaceError("max_response_bytes is outside the protocol bound")
        return SurfaceConfig(
            project_root=root,
            control_plane=self.control_plane.expanduser().resolve(),
            timeout_seconds=timeout,
            max_response_bytes=self.max_response_bytes,
        )


def _safe_id(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise AgentSurfaceError(f"{label} must be a non-empty string")
    if value in {".", ".."} or any(character in value for character in "/\\"):
        raise AgentSurfaceError(f"{label} is not a bounded identifier")
    return value


def _resource(root: Path, value: Any, label: str, *, must_exist: bool = True) -> Path:
    if not isinstance(value, str) or not value:
        raise AgentSurfaceError(f"{label} must be a relative resource path")
    path = Path(value)
    if path.is_absolute() or "\\" in value or any(part == ".." for part in path.parts):
        raise AgentSurfaceError(f"{label} must stay under project_root")
    resolved = (root / path).resolve(strict=must_exist)
    try:
        resolved.relative_to(root)
    except ValueError as error:
        raise AgentSurfaceError(f"{label} escapes project_root") from error
    return resolved


def _json_bytes(value: Mapping[str, Any]) -> bytes:
    try:
        encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    except (TypeError, ValueError) as error:
        raise AgentSurfaceError(f"request is not JSON-serializable: {error}") from error
    if len(encoded) > MAX_REQUEST_BYTES:
        raise AgentSurfaceError("request exceeds the protocol byte bound")
    return encoded


def _capabilities(params: Mapping[str, Any]) -> list[str]:
    capabilities = params.get("capabilities")
    if not isinstance(capabilities, list) or not all(
        isinstance(value, str) and value.strip() for value in capabilities
    ):
        raise AgentSurfaceError("operation requires a bounded capabilities list")
    return sorted(set(capabilities))


@dataclass
class LocalAgentSurface:
    config: SurfaceConfig
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run

    def __post_init__(self) -> None:
        self.config = self.config.validated()

    def capabilities(self) -> dict[str, Any]:
        return {
            "schema_version": PROTOCOL_SCHEMA,
            "transport": "local-json-rpc",
            "semantic_authority": "wge-control-plane",
            "operations": list(OPERATIONS),
            "native_operations": list(OPERATIONS),
            "deferred_operations": [],
            "deferred_gates": [
                "rigging",
                "unity_import",
                "unity_build",
                "unity_playthrough",
            ],
            "limits": {
                "max_request_bytes": MAX_REQUEST_BYTES,
                "max_response_bytes": self.config.max_response_bytes,
                "max_timeout_seconds": MAX_TIMEOUT_SECONDS,
            },
            "mcp": {
                "protocol_version": MCP_PROTOCOL_VERSION,
                "server_name": MCP_SERVER_NAME,
                "transport": "stdio-json-rpc",
                "tool_namespace": "wge_",
            },
        }

    def handle_json_rpc(self, request: Mapping[str, Any]) -> dict[str, Any]:
        request_id = None
        try:
            if not isinstance(request, Mapping):
                raise AgentSurfaceError("request must be an object")
            # Bound the complete JSON-RPC envelope, including capabilities
            # requests and IDs. Checking only `params` would let a caller
            # bypass the protocol ceiling with an oversized envelope.
            _json_bytes(request)
            request_id = request.get("id")
            if request.get("jsonrpc") != "2.0":
                raise AgentSurfaceError("jsonrpc must be \"2.0\"")
            method = request.get("method")
            if not isinstance(method, str):
                raise AgentSurfaceError("method must be a string")
            if method in {"wge/capabilities", "wge.capabilities"}:
                result = self.capabilities()
            else:
                operation = method.removeprefix("wge/").removeprefix("wge.")
                if operation not in OPERATIONS:
                    raise AgentSurfaceError(f"unknown WGE operation {operation!r}")
                params = request.get("params", {})
                if not isinstance(params, Mapping):
                    raise AgentSurfaceError("params must be an object")
                result = self.dispatch(operation, params)
            return {"jsonrpc": "2.0", "id": request_id, "result": result}
        except (AgentSurfaceError, AttributeError) as error:
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": "wge.invalid_request", "message": str(error)},
            }

    def dispatch(self, operation: str, params: Mapping[str, Any]) -> dict[str, Any]:
        _json_bytes(params)
        root = _resource(self.config.project_root, params.get("root", "."), "root")
        candidate = params.get("candidate")
        if candidate is not None:
            candidate = _safe_id(candidate, "candidate")
        command = self._command_for(operation, root, params, candidate)
        result = self._run_native(command)
        if not isinstance(result, dict):
            raise AgentSurfaceError("native control plane returned a non-object result")
        return result

    def _command_for(
        self,
        operation: str,
        root: Path,
        params: Mapping[str, Any],
        candidate: str | None,
    ) -> list[str]:
        binary = str(self.config.control_plane)
        if operation == "inspect_project":
            return [binary, "inspect-project", str(root)]
        if operation == "inspect_current":
            return [binary, "inspect-current", str(root)]
        if operation == "inspect_failures":
            if candidate is None:
                raise AgentSurfaceError("inspect_failures requires candidate")
            return [binary, "inspect-failures", str(root), "--candidate", candidate]
        if operation == "inspect_artifact":
            if candidate is None:
                raise AgentSurfaceError("inspect_artifact requires candidate")
            artifact = _safe_id(params.get("artifact"), "artifact")
            return [binary, "inspect-artifact", str(root), "--candidate", candidate, "--artifact", artifact]
        if operation == "propose_work":
            work_order = _resource(root, params.get("work_order"), "work_order")
            capabilities = _capabilities(params)
            return [
                binary,
                "propose-work",
                str(root),
                "--work-order",
                str(work_order),
                "--capabilities",
                ",".join(capabilities),
            ]
        if operation == "build_candidate":
            manifest = _resource(root, params.get("manifest"), "manifest")
            artifact_root = _resource(root, params.get("artifact_root"), "artifact_root")
            return [
                binary,
                "build-candidate",
                str(root),
                "--manifest",
                str(manifest),
                "--artifact-root",
                str(artifact_root),
            ]
        if operation == "attach_evidence":
            if candidate is None:
                raise AgentSurfaceError("attach_evidence requires candidate")
            request = _resource(root, params.get("request"), "request")
            return [
                binary,
                "attach-evidence",
                str(root),
                "--candidate",
                candidate,
                "--request",
                str(request),
            ]
        if operation == "run_playtest":
            if candidate is None:
                raise AgentSurfaceError("run_playtest requires candidate")
            world_artifact = _safe_id(params.get("world_artifact"), "world_artifact")
            return [
                binary,
                "run-playtest",
                str(root),
                "--candidate",
                candidate,
                "--world-artifact",
                world_artifact,
            ]
        if operation == "capture_evidence":
            if candidate is None:
                raise AgentSurfaceError("capture_evidence requires candidate")
            world_artifact = _safe_id(params.get("world_artifact"), "world_artifact")
            output = _resource(root, params.get("output"), "output", must_exist=False)
            output_relative = output.relative_to(root).as_posix()
            return [
                binary,
                "capture-evidence",
                str(root),
                "--candidate",
                candidate,
                "--world-artifact",
                world_artifact,
                "--output",
                output_relative,
            ]
        if operation in {"evaluate_candidate", "verify_candidate"}:
            if candidate is None:
                raise AgentSurfaceError(f"{operation} requires candidate")
            return [binary, operation.replace("_", "-"), str(root), "--candidate", candidate]
        if operation == "propose_repair":
            if candidate is None:
                raise AgentSurfaceError("propose_repair requires candidate")
            return [binary, "propose-repair", str(root), "--candidate", candidate]
        if operation == "apply_repair":
            return self._work_order_command(binary, "apply-repair", root, params)
        if operation == "commit_candidate":
            if candidate is None:
                raise AgentSurfaceError("commit_candidate requires candidate")
            return [binary, "commit-candidate", str(root), "--candidate", candidate]
        if operation == "rollback":
            snapshot = _safe_id(params.get("snapshot"), "snapshot")
            return [binary, "rollback", str(root), "--snapshot", snapshot]
        if operation == "apply_work_order":
            return self._work_order_command(binary, "execute-work-order", root, params)
        raise AgentSurfaceError(f"no native mapping for {operation}")

    def mcp_tools(self) -> list[dict[str, Any]]:
        """Describe the bounded semantic verbs as MCP tools.

        The descriptions intentionally expose no repository choreography.  A
        client learns the operation vocabulary and typed resource arguments;
        the Rust control plane remains the only semantic authority.
        """

        descriptions = {
            "inspect_project": "Inspect the native WGE project transaction and current pointer.",
            "inspect_current": "Inspect the currently certified native snapshot.",
            "inspect_artifact": "Revalidate and inspect one candidate artifact.",
            "inspect_failures": "Read independently validated failures for a candidate.",
            "propose_work": "Authorize a bounded work order without executing it.",
            "apply_work_order": "Submit a bounded worker result for native verification.",
            "build_candidate": "Copy and identity-check a candidate into native transaction storage.",
            "attach_evidence": "Attach a typed certification request bound to a stored candidate.",
            "run_playtest": "Run and revalidate deterministic reference traversal.",
            "capture_evidence": "Produce deterministic reference visual evidence and provenance.",
            "evaluate_candidate": "Run all registered native validators against a candidate.",
            "propose_repair": "Diagnose failures and emit an authorized-repair request.",
            "apply_repair": "Submit a bounded repair work result for native verification.",
            "verify_candidate": "Re-run native certification verification.",
            "commit_candidate": "Advance the current pointer only after certification passes.",
            "rollback": "Move the pointer to an existing certified history snapshot.",
        }
        tools: list[dict[str, Any]] = []
        for operation in OPERATIONS:
            properties: dict[str, Any] = {
                "root": {
                    "type": "string",
                    "description": "Project resource root relative to the configured WGE workspace.",
                }
            }
            required = ["root"]
            if operation in {
                "inspect_artifact",
                "inspect_failures",
                "run_playtest",
                "capture_evidence",
                "evaluate_candidate",
                "propose_repair",
                "verify_candidate",
                "commit_candidate",
            }:
                properties["candidate"] = {"type": "string"}
                required.append("candidate")
            if operation == "inspect_artifact":
                properties["artifact"] = {"type": "string"}
                required.append("artifact")
            if operation in {"run_playtest", "capture_evidence"}:
                properties["world_artifact"] = {"type": "string"}
                required.append("world_artifact")
            if operation == "capture_evidence":
                properties["output"] = {"type": "string"}
                required.append("output")
            if operation == "build_candidate":
                properties["manifest"] = {"type": "string"}
                properties["artifact_root"] = {"type": "string"}
                required.extend(["manifest", "artifact_root"])
            if operation == "attach_evidence":
                properties["candidate"] = {"type": "string"}
                properties["request"] = {"type": "string"}
                required.extend(["candidate", "request"])
            if operation == "propose_work":
                properties["work_order"] = {"type": "string"}
                properties["capabilities"] = {"type": "array", "items": {"type": "string"}}
                required.extend(["work_order", "capabilities"])
            if operation in {"apply_work_order", "apply_repair"}:
                properties["work_order"] = {"type": "string"}
                properties["result"] = {"type": "string"}
                properties["capabilities"] = {"type": "array", "items": {"type": "string"}}
                required.extend(["work_order", "result", "capabilities"])
            if operation == "rollback":
                properties["snapshot"] = {"type": "string"}
                required.append("snapshot")
            tools.append(
                {
                    "name": f"wge_{operation}",
                    "description": descriptions[operation],
                    "inputSchema": {
                        "type": "object",
                        "properties": properties,
                        "required": required,
                        "additionalProperties": False,
                    },
                }
            )
        return tools

    def handle_mcp_json_rpc(self, request: Mapping[str, Any]) -> dict[str, Any] | None:
        """Handle the minimal MCP stdio JSON-RPC surface.

        This is deliberately a schema/transport adapter.  Tool calls are
        delegated to :meth:`dispatch`; no MCP method is allowed to inspect or
        mutate WGE state directly.
        """

        request_id = request.get("id") if isinstance(request, Mapping) else None
        try:
            if not isinstance(request, Mapping):
                raise AgentSurfaceError("request must be an object")
            _json_bytes(request)
            if request.get("jsonrpc") != "2.0":
                raise AgentSurfaceError('jsonrpc must be "2.0"')
            method = request.get("method")
            if not isinstance(method, str):
                raise AgentSurfaceError("method must be a string")
            if method == "notifications/initialized":
                return None
            if method == "ping":
                result: dict[str, Any] = {}
            elif method == "initialize":
                result = {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {"tools": {"listChanged": False}},
                    "serverInfo": {"name": MCP_SERVER_NAME, "version": "1"},
                    "instructions": "Use the bounded wge_* semantic tools; Rust owns validation and promotion.",
                }
            elif method == "tools/list":
                result = {"tools": self.mcp_tools(), "nextCursor": None}
            elif method == "tools/call":
                params = request.get("params", {})
                if not isinstance(params, Mapping):
                    raise AgentSurfaceError("tools/call params must be an object")
                name = params.get("name")
                if not isinstance(name, str) or not name.startswith("wge_"):
                    raise AgentSurfaceError("tools/call name must be a registered wge_ tool")
                operation = name.removeprefix("wge_")
                if operation not in OPERATIONS:
                    raise AgentSurfaceError(f"unknown WGE tool {name!r}")
                arguments = params.get("arguments", {})
                if not isinstance(arguments, Mapping):
                    raise AgentSurfaceError("tools/call arguments must be an object")
                try:
                    value = self.dispatch(operation, arguments)
                except AgentSurfaceError as error:
                    # A recognized tool that cannot complete is an MCP tool
                    # result, not a JSON-RPC protocol failure. This lets a
                    # model inspect and repair native failures without losing
                    # the request/response session.
                    value = {"error": str(error)}
                    result = {
                        "content": [{"type": "text", "text": json.dumps(value, sort_keys=True)}],
                        "structuredContent": value,
                        "isError": True,
                    }
                else:
                    result = {
                        "content": [{"type": "text", "text": json.dumps(value, sort_keys=True)}],
                        "structuredContent": value,
                        "isError": False,
                    }
            else:
                raise AgentSurfaceError(f"unsupported MCP method {method!r}")
            return {"jsonrpc": "2.0", "id": request_id, "result": result}
        except AgentSurfaceError as error:
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": "wge.invalid_request", "message": str(error)},
            }

    @staticmethod
    def _work_order_command(
        binary: str, operation: str, root: Path, params: Mapping[str, Any]
    ) -> list[str]:
        work_order = _resource(root, params.get("work_order"), "work_order")
        result = _resource(root, params.get("result"), "result")
        capabilities = _capabilities(params)
        return [
            binary,
            operation,
            str(root),
            "--work-order",
            str(work_order),
            "--result",
            str(result),
            "--capabilities",
            ",".join(capabilities),
        ]

    def _run_native(self, command: Sequence[str]) -> dict[str, Any]:
        if not self.config.control_plane.is_file():
            raise AgentSurfaceError(f"native control plane is unavailable: {self.config.control_plane}")
        try:
            result = self.runner(
                list(command),
                cwd=str(self.config.project_root),
                text=True,
                capture_output=True,
                timeout=self.config.timeout_seconds,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise AgentSurfaceError(f"native control-plane transport failed: {error}") from error
        output = (result.stdout or "").encode()
        if len(output) > self.config.max_response_bytes:
            raise AgentSurfaceError("native response exceeds the protocol byte bound")
        if result.returncode != 0:
            detail = (result.stderr or result.stdout or "native operation failed").strip()
            raise AgentSurfaceError(f"native control-plane operation failed: {detail}")
        try:
            value = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise AgentSurfaceError(f"native control plane returned malformed JSON: {error}") from error
        if not isinstance(value, dict):
            raise AgentSurfaceError("native control plane response must be an object")
        return value


def serve_stdio(surface: LocalAgentSurface) -> None:
    """Serve the WGE-native JSON-RPC compatibility protocol over stdin/stdout."""

    for line in os.sys.stdin:
        if len(line.encode()) > MAX_REQUEST_BYTES:
            response = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": "wge.invalid_request", "message": "request exceeds byte bound"},
            }
        else:
            try:
                value = json.loads(line)
                response = surface.handle_json_rpc(value)
            except (TypeError, ValueError, json.JSONDecodeError) as error:
                response = {
                    "jsonrpc": "2.0",
                    "id": None,
                    "error": {"code": "wge.invalid_request", "message": str(error)},
                }
        os.sys.stdout.write(json.dumps(response, sort_keys=True) + "\n")
        os.sys.stdout.flush()


def serve_mcp_stdio(surface: LocalAgentSurface) -> None:
    """Serve the MCP-compatible semantic tool adapter over newline JSON-RPC."""

    for line in os.sys.stdin:
        if len(line.encode()) > MAX_REQUEST_BYTES:
            response: dict[str, Any] | None = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": "wge.invalid_request", "message": "request exceeds byte bound"},
            }
        else:
            try:
                value = json.loads(line)
                response = surface.handle_mcp_json_rpc(value)
            except (TypeError, ValueError, json.JSONDecodeError) as error:
                response = {
                    "jsonrpc": "2.0",
                    "id": None,
                    "error": {"code": "wge.invalid_request", "message": str(error)},
                }
        # MCP notifications intentionally have no response.  Do not emit a
        # synthetic line that could desynchronize a strict client.
        if response is not None:
            encoded = json.dumps(response, sort_keys=True)
            if len(encoded.encode()) > surface.config.max_response_bytes:
                encoded = json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": None,
                        "error": {
                            "code": "wge.response_too_large",
                            "message": "MCP response exceeds the configured byte bound",
                        },
                    },
                    sort_keys=True,
                )
            os.sys.stdout.write(encoded + "\n")
            os.sys.stdout.flush()


def _main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="WGE-native MCP/agent surface")
    parser.add_argument("--project-root", type=Path, required=True)
    parser.add_argument(
        "--control-plane",
        type=Path,
        default=ROOT / "world_core" / "target" / "debug" / "wge-control-plane",
    )
    parser.add_argument("--legacy-json-rpc", action="store_true")
    args = parser.parse_args(argv)
    surface = LocalAgentSurface(SurfaceConfig(args.project_root, args.control_plane))
    if args.legacy_json_rpc:
        serve_stdio(surface)
    else:
        serve_mcp_stdio(surface)
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
