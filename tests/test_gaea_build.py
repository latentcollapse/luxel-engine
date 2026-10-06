"""The Gaea build driver and its MCP surface.

The live build is not exercised here (it needs Gaea under Proton); the runner
is injected. What is tested is everything the live runs on 2026-10-05 showed
the driver must get right on its own:

- Success is decided by files appearing, never by Swarm's exit code.
- Receipts digest decoded pixels, because Gaea stamps creation times into its
  PNGs and the file digest changes on every build.
- Nothing that reaches the three-layer `script -> proton -> cmd.exe` command
  line can carry a space or a shell metacharacter.
- Staging is cleaned up on failure as well as success.
- Graph edits through MCP never write in place or into Gaea's examples.
"""

from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy as np
from PIL import Image, PngImagePlugin

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import gaea_build  # noqa: E402
import gaea_mcp_server as server  # noqa: E402
import gaea_terrain  # noqa: E402


def mini_graph() -> dict:
    """Mountain (10) -> Erosion2 (11), in the keyed `Nodes` layout Gaea writes."""
    return {
        "$id": "1",
        "Assets": {
            "$id": "2",
            "$values": [
                {
                    "$id": "3",
                    "Terrain": {
                        "$id": "4",
                        "Nodes": {
                            "$id": "5",
                            "10": {
                                "$id": "6",
                                "$type": "QuadSpinner.Gaea.Nodes.Mountain, Gaea.Nodes",
                                "Id": 10,
                                "Name": "Mountain",
                                "Position": {"$id": "7", "X": 0.0, "Y": 0.0},
                                "Ports": {"$id": "8", "$values": [
                                    {"$id": "9", "Name": "Out", "Type": "PrimaryOut"},
                                ]},
                            },
                            "11": {
                                "$id": "12",
                                "$type": "QuadSpinner.Gaea.Nodes.Erosion2, Gaea.Nodes",
                                "Id": 11,
                                "Name": "My renamed erosion",
                                "Position": {"$id": "13", "X": 1.0, "Y": 0.0},
                                "Ports": {"$id": "14", "$values": [
                                    {"$id": "15", "Name": "In", "Type": "PrimaryIn, Required",
                                     "Record": {"$id": "16", "From": 10, "To": 11,
                                                "FromPort": "Out", "ToPort": "In"}},
                                    {"$id": "17", "Name": "Out", "Type": "PrimaryOut"},
                                ]},
                            },
                        },
                        "Variables": {"$id": "18"},
                    },
                    "BuildDefinition": {"$id": "19", "Type": "Standard", "Resolution": 2048},
                }
            ],
        },
    }


def write_png16(path: Path, data: np.ndarray, stamp: str = "") -> None:
    info = PngImagePlugin.PngInfo()
    if stamp:
        info.add_text("date:create", stamp)
    Image.fromarray(data.astype(np.uint16)).save(path, pnginfo=info)


class FakeEnvironment(gaea_build.GaeaEnvironment):
    def check(self) -> list[str]:
        return []


class DriverTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        gaea_dir = self.tmp / "gaea2"
        gaea_dir.mkdir()
        self.environment = FakeEnvironment(
            gaea_dir=gaea_dir,
            proton=self.tmp / "Proton - Experimental" / "proton",
            steam_root=self.tmp / "steam",
            compat_data=self.tmp / "steam" / "compatdata" / "gaea-swarm",
        )
        self.graph = self.tmp / "graph with spaces.terrain"
        graph = mini_graph()
        gaea_terrain.add_save_definition(graph, 11, "Height")
        self.graph.write_text(json.dumps(graph))

    def request(self, **overrides) -> gaea_build.BuildRequest:
        values = dict(graph=self.graph, output_dir=self.tmp / "out dir", resolution=512, seed=7)
        values.update(overrides)
        return gaea_build.BuildRequest(**values)

    # --- command line ---------------------------------------------------

    def test_windows_path_converts_and_refuses_spaces(self) -> None:
        self.assertEqual(gaea_build.windows_path(Path("/home/x/a.terrain")), "Z:\\home\\x\\a.terrain")
        with self.assertRaises(gaea_build.GaeaBuildError):
            gaea_build.windows_path(Path("/home/x/a b.terrain"))
        with self.assertRaises(gaea_build.GaeaBuildError):
            gaea_build.windows_path(Path("/home/x/a&b.terrain"))

    def test_command_runs_under_a_pty_with_silent_and_no_redirect(self) -> None:
        staged = self.environment.staging / "s" / "graph.terrain"
        argv = gaea_build.swarm_command(self.environment, staged, staged.parent / "out", self.request())
        self.assertEqual(argv[:2], ["script", "-qec"])
        self.assertEqual(argv[3], "/dev/null")
        inner = argv[2]
        self.assertIn("run cmd.exe /c", inner)
        self.assertIn("--silent", inner)
        self.assertIn("--ignorecache", inner)
        self.assertIn("--resolution 512", inner)
        self.assertIn("--seed 7", inner)
        self.assertNotIn(">", inner)
        # The proton path's space is quoted for the shell, nothing else is.
        self.assertTrue(inner.startswith("'" + str(self.environment.proton) + "'"))

    def test_variables_are_passed_and_unsafe_ones_refused(self) -> None:
        staged = self.environment.staging / "s" / "graph.terrain"
        argv = gaea_build.swarm_command(
            self.environment, staged, staged.parent / "out",
            self.request(variables={"Height": "0.75", "Scale": "2"}),
        )
        self.assertIn("-v Height=0.75 -v Scale=2", argv[2])
        for name, value in (("a b", "1"), ("x", "1;rm"), ("x", 'a"b'), ("x", "a&b"), ("1x", "1")):
            with self.assertRaises(gaea_build.GaeaBuildError, msg=f"{name}={value}"):
                gaea_build.swarm_command(
                    self.environment, staged, staged.parent / "out",
                    self.request(variables={name: value}),
                )

    def test_resolution_and_seed_are_bounded(self) -> None:
        staged = self.environment.staging / "s" / "graph.terrain"
        with self.assertRaises(gaea_build.GaeaBuildError):
            gaea_build.swarm_command(self.environment, staged, staged, self.request(resolution=1025))
        with self.assertRaises(gaea_build.GaeaBuildError):
            gaea_build.swarm_command(self.environment, staged, staged, self.request(seed=2**31))

    # --- build verification -------------------------------------------

    def fake_runner(self, write: bool, code: int | None = 0):
        calls = []

        def runner(argv, cwd, env, timeout_s):
            calls.append((argv, cwd, env))
            if write:
                build_path = Path(argv[2].split("--buildpath ")[1].split(" ")[0].replace("Z:", "").replace("\\", "/"))
                write_png16(build_path / "Height_Out.png", np.arange(64).reshape(8, 8) * 1000, stamp="t1")
            return code, 1.5

        runner.calls = calls
        return runner

    def test_success_is_files_not_exit_code(self) -> None:
        # Exit 1 with files written is a success; exit 0 with nothing is not.
        receipt = gaea_build.build(self.request(), self.environment, self.fake_runner(write=True, code=1))
        self.assertEqual(receipt["swarm_exit_code"], 1)
        with self.assertRaises(gaea_build.GaeaBuildError) as caught:
            gaea_build.build(self.request(), self.environment, self.fake_runner(write=False, code=0))
        self.assertIn("wrote nothing", str(caught.exception))

    def test_timeout_with_partial_files_fails(self) -> None:
        with self.assertRaises(gaea_build.GaeaBuildError) as caught:
            gaea_build.build(self.request(), self.environment, self.fake_runner(write=True, code=None))
        self.assertIn("timed out", str(caught.exception))

    def test_staging_is_space_free_and_removed_on_success_and_failure(self) -> None:
        runner = self.fake_runner(write=True)
        gaea_build.build(self.request(), self.environment, runner)
        argv, cwd, env = runner.calls[0]
        self.assertEqual(cwd, self.environment.gaea_dir)
        self.assertEqual(env["STEAM_COMPAT_DATA_PATH"], str(self.environment.compat_data))
        self.assertIn("graph.terrain", argv[2])
        self.assertNotIn("graph with spaces", argv[2])
        self.assertEqual(list(self.environment.staging.iterdir()), [])
        with self.assertRaises(gaea_build.GaeaBuildError):
            gaea_build.build(self.request(), self.environment, self.fake_runner(write=False))
        self.assertEqual(list(self.environment.staging.iterdir()), [])

    def test_preflight_refuses_graphs_swarm_would_silently_skip(self) -> None:
        unmarked = self.tmp / "unmarked.terrain"
        unmarked.write_text(json.dumps(mini_graph()))
        untyped_graph = mini_graph()
        gaea_terrain.add_save_definition(untyped_graph, 11, "Height")
        del untyped_graph["Assets"]["$values"][0]["BuildDefinition"]["Type"]
        untyped = self.tmp / "untyped.terrain"
        untyped.write_text(json.dumps(untyped_graph))
        self.assertEqual(gaea_build.preflight(self.graph), [])
        for path, expected in ((unmarked, "SaveDefinition"), (untyped, "no Type")):
            runner = self.fake_runner(write=True)
            with self.assertRaises(gaea_build.GaeaBuildError) as caught:
                gaea_build.build(self.request(graph=path), self.environment, runner)
            self.assertIn(expected, str(caught.exception))
            self.assertEqual(runner.calls, [], "Swarm must not be launched for an unbuildable graph")

    def test_same_second_builds_get_distinct_staging(self) -> None:
        seen = []

        def runner(argv, cwd, env, timeout_s):
            seen.append(argv[2].split("--buildpath ")[1].split(" ")[0])
            return self.fake_runner(write=True)(argv, cwd, env, timeout_s)

        with mock.patch.object(gaea_build.time, "strftime", return_value="20261005T000000"):
            gaea_build.build(self.request(output_dir=self.tmp / "o1"), self.environment, runner)
            gaea_build.build(self.request(output_dir=self.tmp / "o2"), self.environment, runner)
        self.assertEqual(len(set(seen)), 2)

    def test_receipt_pins_graph_request_and_pixels(self) -> None:
        receipt = gaea_build.build(
            self.request(variables={"Height": "0.5"}), self.environment, self.fake_runner(write=True)
        )
        self.assertEqual(receipt["schema"], gaea_build.RECEIPT_SCHEMA)
        self.assertEqual(receipt["graph"]["sha256"], gaea_build.sha256_file(self.graph))
        # Types come from $type, not the user-editable Name.
        self.assertEqual(receipt["graph"]["node_types"], ["Erosion2", "Mountain"])
        self.assertFalse(receipt["byte_reproducible_expected"])
        self.assertEqual(receipt["request"], {
            "resolution": 512, "seed": 7, "variables": {"Height": "0.5"}, "ignore_cache": True,
        })
        (output,) = receipt["outputs"]
        self.assertEqual(output["path"], "Height_Out.png")
        self.assertEqual(output["mode"], "I;16")
        self.assertEqual(output["shape"], [8, 8])
        self.assertEqual((output["min"], output["max"]), (0, 63000))
        on_disk = json.loads((self.tmp / "out dir" / "gaea-build-receipt.json").read_text())
        self.assertEqual(on_disk, receipt)

    def test_pixel_digest_ignores_png_timestamps(self) -> None:
        data = np.arange(16).reshape(4, 4) * 4000
        write_png16(self.tmp / "a.png", data, stamp="2026-10-06T01:51:02")
        write_png16(self.tmp / "b.png", data, stamp="2026-10-06T01:51:15")
        write_png16(self.tmp / "c.png", data + 1, stamp="2026-10-06T01:51:02")
        self.assertNotEqual(gaea_build.sha256_file(self.tmp / "a.png"), gaea_build.sha256_file(self.tmp / "b.png"))
        a, b, c = (gaea_build.pixel_digest(self.tmp / n) for n in ("a.png", "b.png", "c.png"))
        self.assertEqual(a["pixel_sha256"], b["pixel_sha256"])
        self.assertNotEqual(a["pixel_sha256"], c["pixel_sha256"])

    def test_compare_reports_identity_and_tolerance(self) -> None:
        for name in ("one", "two", "three"):
            (self.tmp / name).mkdir()
        base = np.full((4, 4), 1000)
        write_png16(self.tmp / "one" / "Height_Out.png", base, stamp="x")
        write_png16(self.tmp / "two" / "Height_Out.png", base, stamp="y")
        shifted = base.copy()
        shifted[0, 0] += 113
        write_png16(self.tmp / "three" / "Height_Out.png", shifted)
        same = gaea_build.compare(self.tmp / "one", self.tmp / "two")
        self.assertTrue(same["pixel_identical"])
        different = gaea_build.compare(self.tmp / "one", self.tmp / "three")
        self.assertFalse(different["pixel_identical"])
        (row,) = different["outputs"]
        self.assertEqual(row["max_abs_diff"], 113)
        self.assertEqual(row["differing_fraction"], 1 / 16)
        self.assertFalse(gaea_build.compare(self.tmp / "one", self.tmp / "missing")["pixel_identical"])


class BuildTypeTest(unittest.TestCase):
    def test_marking_an_export_fills_a_missing_build_type(self) -> None:
        graph = mini_graph()
        definition = graph["Assets"]["$values"][0]["BuildDefinition"]
        del definition["Type"]
        gaea_terrain.add_save_definition(graph, 11, "Height")
        self.assertEqual(definition["Type"], "Standard")
        self.assertEqual(list(definition)[:2], ["$id", "Type"])
        self.assertEqual(definition["Resolution"], 2048)

    def test_an_existing_build_type_is_kept(self) -> None:
        graph = mini_graph()
        graph["Assets"]["$values"][0]["BuildDefinition"]["Type"] = "Tiled"
        self.assertEqual(gaea_terrain.ensure_build_type(graph), 0)
        self.assertEqual(graph["Assets"]["$values"][0]["BuildDefinition"]["Type"], "Tiled")


class LoadGraphTest(unittest.TestCase):
    def test_trailing_commas_are_accepted_and_strings_untouched(self) -> None:
        text = '{"a": [1, 2,], "b": {"c": "x,}", "d": "y, ]",\n },}'
        with tempfile.NamedTemporaryFile("w", suffix=".terrain", delete=False) as handle:
            handle.write("\ufeff" + text)
        try:
            self.assertEqual(
                gaea_terrain.load_graph(Path(handle.name)),
                {"a": [1, 2], "b": {"c": "x,}", "d": "y, ]"}},
            )
        finally:
            os.unlink(handle.name)

    def test_escaped_quotes_do_not_end_strings(self) -> None:
        text = r'{"a": "q\",}", "b": 1,}'
        self.assertEqual(json.loads(gaea_terrain._strip_trailing_commas(text)), {"a": 'q",}', "b": 1})


class McpServerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.examples = self.tmp / "gaea2" / "Examples"
        self.examples.mkdir(parents=True)
        self.graph = self.tmp / "work" / "ridge.terrain"
        self.graph.parent.mkdir()
        self.graph.write_text(json.dumps(mini_graph()))
        environment = FakeEnvironment(
            gaea_dir=self.tmp / "gaea2",
            proton=self.tmp / "proton",
            steam_root=self.tmp,
            compat_data=self.tmp / "compat",
        )
        patcher = mock.patch.object(server, "_environment", return_value=environment)
        patcher.start()
        self.addCleanup(patcher.stop)

    def call(self, name: str, arguments: dict) -> tuple[bool, object]:
        response = server.handle(
            {"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": name, "arguments": arguments}}
        )
        result = response["result"]
        text = result["content"][0]["text"]
        return result["isError"], (text if result["isError"] else json.loads(text))

    def test_initialize_and_tools_list(self) -> None:
        init = server.handle({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                              "params": {"protocolVersion": "2025-06-18"}})
        self.assertEqual(init["result"]["protocolVersion"], "2025-06-18")
        self.assertIn("tools", init["result"]["capabilities"])
        self.assertIsNone(server.handle({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        tools = server.handle({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})["result"]["tools"]
        self.assertEqual(
            {tool["name"] for tool in tools},
            {"gaea_status", "gaea_list_examples", "gaea_inspect_graph", "gaea_mark_export",
             "gaea_insert_node", "gaea_build", "gaea_compare_builds"},
        )
        for tool in tools:
            self.assertEqual(tool["inputSchema"]["type"], "object")

    def test_inspect_reports_wiring_terminals_and_buildability(self) -> None:
        error, payload = self.call("gaea_inspect_graph", {"path": str(self.graph)})
        self.assertFalse(error)
        self.assertEqual(payload["terminal_nodes"], [11])
        self.assertFalse(payload["buildable"])
        erosion = next(node for node in payload["nodes"] if node["id"] == 11)
        self.assertEqual(erosion["type"], "Erosion2")
        self.assertEqual(erosion["inputs"], [{"port": "In", "from": 10, "from_port": "Out"}])

    def test_mark_export_writes_a_copy_and_makes_it_buildable(self) -> None:
        output = self.tmp / "work" / "ridge_export.terrain"
        error, payload = self.call(
            "gaea_mark_export", {"path": str(self.graph), "node_id": 11, "output_path": str(output)}
        )
        self.assertFalse(error, payload)
        self.assertEqual(json.loads(self.graph.read_text()), mini_graph())  # input untouched
        _, inspected = self.call("gaea_inspect_graph", {"path": str(output)})
        self.assertTrue(inspected["buildable"])
        self.assertEqual(inspected["exports"], [11])

    def test_edits_refuse_in_place_examples_and_unknown_nodes(self) -> None:
        cases = [
            {"path": str(self.graph), "node_id": 11, "output_path": str(self.graph)},
            {"path": str(self.graph), "node_id": 11, "output_path": str(self.examples / "x.terrain")},
            {"path": str(self.graph), "node_id": 11, "output_path": str(self.tmp / "x.json")},
            {"path": str(self.graph), "node_id": 999, "output_path": str(self.tmp / "y.terrain")},
            {"path": str(self.graph), "output_path": str(self.tmp / "z.terrain")},
        ]
        for arguments in cases:
            error, text = self.call("gaea_mark_export", arguments)
            self.assertTrue(error, arguments)
        self.assertFalse((self.examples / "x.terrain").exists())

    def test_insert_node_rewires(self) -> None:
        graph = mini_graph()
        # Add a consumer of 11 so the splice has something to rewire.
        nodes = graph["Assets"]["$values"][0]["Terrain"]["Nodes"]
        nodes["12"] = {"$id": "40", "$type": "QuadSpinner.Gaea.Nodes.Snowfield, Gaea.Nodes", "Id": 12,
                       "Name": "Snowfield", "Position": {"$id": "41", "X": 2.0, "Y": 0.0},
                       "Ports": {"$id": "42", "$values": [
                           {"$id": "43", "Name": "In", "Type": "PrimaryIn",
                            "Record": {"$id": "44", "From": 11, "To": 12, "FromPort": "Out", "ToPort": "In"}}]}}
        self.graph.write_text(json.dumps(graph))
        output = self.tmp / "work" / "spliced.terrain"
        error, payload = self.call(
            "gaea_insert_node",
            {"path": str(self.graph), "after_node_id": 11, "node_type": "Erosion2", "output_path": str(output)},
        )
        self.assertFalse(error, payload)
        self.assertEqual(payload["rewired_consumers"], 1)
        _, inspected = self.call("gaea_inspect_graph", {"path": str(output)})
        snow = next(node for node in inspected["nodes"] if node["id"] == 12)
        self.assertEqual(snow["inputs"][0]["from"], payload["node_id"])

    def test_build_failure_is_a_tool_error_not_a_crash(self) -> None:
        with mock.patch.object(gaea_build, "build", side_effect=gaea_build.GaeaBuildError("wrote nothing")):
            error, text = self.call("gaea_build", {"graph": str(self.graph), "output_dir": str(self.tmp / "o")})
        self.assertTrue(error)
        self.assertIn("wrote nothing", text)

    def test_unknown_tool_and_method(self) -> None:
        response = server.handle({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "nope"}})
        self.assertEqual(response["error"]["code"], -32602)
        response = server.handle({"jsonrpc": "2.0", "id": 4, "method": "resources/list"})
        self.assertEqual(response["error"]["code"], -32601)

    def test_serve_loop_is_newline_delimited(self) -> None:
        stdin = io.StringIO(
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping"}) + "\n"
            + "\n"
            + "not json\n"
            + json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        stdout = io.StringIO()
        server.serve(stdin, stdout)
        lines = [json.loads(line) for line in stdout.getvalue().splitlines()]
        self.assertEqual(lines[0], {"jsonrpc": "2.0", "id": 1, "result": {}})
        self.assertEqual(lines[1]["error"]["code"], -32700)
        self.assertEqual(len(lines), 2)


if __name__ == "__main__":
    unittest.main()
