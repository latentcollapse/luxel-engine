from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "pipeline"))

from wge_agent_surface import (  # noqa: E402
    MAX_REQUEST_BYTES,
    LocalAgentSurface,
    MCP_PROTOCOL_VERSION,
    SurfaceConfig,
)


class AgentSurfaceTest(unittest.TestCase):
    def test_capabilities_expose_the_native_operation_surface(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            capabilities = surface.capabilities()
            self.assertEqual(capabilities["schema_version"], "wge.agent-operation/v1")
            self.assertIn("inspect_project", capabilities["operations"])
            self.assertIn("inspect_current", capabilities["operations"])
            self.assertIn("attach_evidence", capabilities["operations"])
            self.assertIn("build_candidate", capabilities["native_operations"])
            self.assertEqual(capabilities["deferred_operations"], [])
            self.assertIn("rigging", capabilities["deferred_gates"])
            self.assertEqual(capabilities["mcp"]["protocol_version"], MCP_PROTOCOL_VERSION)

    def test_request_boundary_rejects_malformed_and_oversized_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            malformed = surface.handle_json_rpc([])  # type: ignore[arg-type]
            self.assertEqual(malformed["error"]["code"], "wge.invalid_request")
            oversized = {"jsonrpc": "2.0", "id": 1, "method": "wge/capabilities", "params": {"x": "x" * MAX_REQUEST_BYTES}}
            response = surface.handle_json_rpc(oversized)
            self.assertIn("request exceeds", response["error"]["message"])
            oversized_id = surface.handle_json_rpc(
                {"jsonrpc": "2.0", "id": "x" * MAX_REQUEST_BYTES, "method": "wge/capabilities"}
            )
            self.assertIn("request exceeds", oversized_id["error"]["message"])
            with self.assertRaisesRegex(RuntimeError, "request exceeds"):
                surface.dispatch("inspect_project", {"payload": "x" * MAX_REQUEST_BYTES})

    def test_resource_paths_cannot_escape_project_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            response = surface.handle_json_rpc(
                {"jsonrpc": "2.0", "id": 4, "method": "wge/inspect_project", "params": {"root": "../escape"}}
            )
            self.assertIn("project_root", response["error"]["message"])

    def test_native_mapping_is_argument_vector_not_shell_text(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            calls: list[list[str]] = []

            def runner(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
                calls.append(command)
                return subprocess.CompletedProcess(command, 0, '{"status":"ok"}\n', "")

            surface = LocalAgentSurface(SurfaceConfig(root, binary), runner=runner)
            result = surface.dispatch("inspect_project", {})
            self.assertEqual(result["status"], "ok")
            self.assertEqual(calls[0][1:], ["inspect-project", str(root)])
            self.assertTrue(all(";" not in part and "&&" not in part for part in calls[0]))

    def test_work_order_mapping_is_a_real_native_argv(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            (root / "order.json").write_text("{}", encoding="utf-8")
            calls: list[list[str]] = []

            def runner(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
                calls.append(command)
                return subprocess.CompletedProcess(command, 0, '{"status":"ok"}\n', "")

            surface = LocalAgentSurface(SurfaceConfig(root, binary), runner=runner)
            surface.dispatch(
                "propose_work",
                {"work_order": "order.json", "capabilities": ["terrain"]},
            )
            self.assertEqual(
                calls[0][1:],
                [
                    "propose-work",
                    str(root),
                    "--work-order",
                    str(root / "order.json"),
                    "--capabilities",
                    "terrain",
                ],
            )

    def test_native_operation_requires_its_typed_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            response = surface.handle_json_rpc(
                {"jsonrpc": "2.0", "id": 9, "method": "wge/build_candidate", "params": {}}
            )
            self.assertIn("manifest", response["error"]["message"])

    def test_mcp_initialize_lists_bounded_semantic_tools(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            initialized = surface.handle_mcp_json_rpc(
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
            )
            assert initialized is not None
            self.assertEqual(initialized["result"]["protocolVersion"], MCP_PROTOCOL_VERSION)
            tools = surface.handle_mcp_json_rpc(
                {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
            )
            assert tools is not None
            names = {tool["name"] for tool in tools["result"]["tools"]}
            self.assertIn("wge_inspect_project", names)
            self.assertIn("wge_attach_evidence", names)
            self.assertTrue(all(tool["inputSchema"]["additionalProperties"] is False for tool in tools["result"]["tools"]))
            by_name = {tool["name"]: tool for tool in tools["result"]["tools"]}
            self.assertEqual(by_name["wge_commit_candidate"]["inputSchema"]["required"], ["root", "candidate"])
            self.assertIn("work_order", by_name["wge_apply_work_order"]["inputSchema"]["required"])
            self.assertIsNone(
                surface.handle_mcp_json_rpc(
                    {"jsonrpc": "2.0", "method": "notifications/initialized"}
                )
            )

    def test_mcp_tool_call_delegates_without_reimplementing_native_semantics(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            calls: list[list[str]] = []

            def runner(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
                calls.append(command)
                return subprocess.CompletedProcess(command, 0, '{"status":"native"}\n', "")

            surface = LocalAgentSurface(SurfaceConfig(root, binary), runner=runner)
            response = surface.handle_mcp_json_rpc(
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": "wge_inspect_current", "arguments": {}},
                }
            )
            assert response is not None
            self.assertFalse(response["result"]["isError"])
            self.assertEqual(response["result"]["structuredContent"]["status"], "native")
            self.assertEqual(calls[0][1:], ["inspect-current", str(root)])

    def test_mcp_tool_execution_failure_is_a_tool_result(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")

            def runner(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
                return subprocess.CompletedProcess(command, 2, "", "candidate is stale")

            surface = LocalAgentSurface(SurfaceConfig(root, binary), runner=runner)
            response = surface.handle_mcp_json_rpc(
                {
                    "jsonrpc": "2.0",
                    "id": 8,
                    "method": "tools/call",
                    "params": {"name": "wge_inspect_current", "arguments": {}},
                }
            )
            assert response is not None
            self.assertTrue(response["result"]["isError"])
            self.assertIn("candidate is stale", response["result"]["structuredContent"]["error"])

    def test_mcp_rejects_unknown_tool_and_oversized_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "wge-control-plane"
            binary.write_bytes(b"control-plane-test-double")
            surface = LocalAgentSurface(SurfaceConfig(root, binary))
            unknown = surface.handle_mcp_json_rpc(
                {
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "tools/call",
                    "params": {"name": "wge_not_a_real_tool", "arguments": {}},
                }
            )
            assert unknown is not None
            self.assertIn("unknown WGE tool", unknown["error"]["message"])
            oversized = surface.handle_mcp_json_rpc(
                {"jsonrpc": "2.0", "id": "x" * MAX_REQUEST_BYTES, "method": "tools/list"}
            )
            assert oversized is not None
            self.assertIn("request exceeds", oversized["error"]["message"])


if __name__ == "__main__":
    unittest.main()
