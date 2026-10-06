#!/usr/bin/env python3
"""Build a Gaea `.terrain` graph headlessly and pin what it produced.

This is the driver from docs/integrations/gaea-programme.md section 8 step 5. It owns the
invocation traps recorded in section 3.3 so no caller has to remember them:

- Swarm only parses a graph given as a full `Z:` path with no spaces in it. The
  graph is copied to a space-free staging name on NVMe before every build.
- Swarm needs a real console (Spectre.Console). It runs under `script -qec`
  and Gaea's own stdout is never redirected.
- Swarm's exit code is not evidence. Exit 0 with no files, exit 0 after a usage
  dump and exit 1 have all been observed. A build succeeded if and only if
  files appeared in its build path.
- Gaea runs from `~/.local/share/gaea2` (NVMe), never from `/mnt/d` (33 s of
  startup per invocation there, D30).

**What the receipt pins, and why it is pixels and not files.** Measured
2026-10-05 against `Snowy Ridge` at 512: Gaea writes `date:create` /
`date:modify` into every PNG, so the file digest differs on every build even
when the pixels are identical. A bare `Mountain` node is pixel-identical across
builds. `Erosion2` is not, with or without `--safemode`: two builds of
Mountain -> Erosion2 -> Snowfield differed by up to 113/65535 in height on 65%
of pixels, and about 4k snow-mask pixels flipped. So the receipt digests the
decoded pixel array, and an eroded build is a **source artifact**: regenerable
within tolerance, never byte-exact. `compare` measures that tolerance.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from PIL import Image

RECEIPT_SCHEMA = "wge.gaea-build-receipt/v1"
COMPARE_SCHEMA = "wge.gaea-build-compare/v1"

# Swarm arguments cross three quoting layers (`script` -> `proton` ->
# `cmd.exe`). Rather than quote through all three, anything that reaches the
# command line is restricted to characters none of them treat specially.
_SAFE_TOKEN = re.compile(r"^[A-Za-z0-9_.:\\/-]+$")
_SAFE_VARIABLE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_SAFE_VALUE = re.compile(r"^[A-Za-z0-9_.+-]+$")

# Gaea's own resolution menu. Swarm is not built for anything else (section 3.1);
# the importer resamples to the viewer's 2**k + 1 grid afterwards.
RESOLUTIONS = (256, 512, 1024, 2048, 4096, 8192)

# Node types measured or known to be nondeterministic between builds. Used only
# to label a receipt; `compare` is the measurement.
NONDETERMINISTIC_NODES = ("Erosion", "Erosion2", "Rivers", "Snowfield", "ThermalShaper")


class GaeaBuildError(RuntimeError):
    """A build that produced no files, timed out, or could not be launched."""


@dataclass(frozen=True)
class GaeaEnvironment:
    """Where Gaea and the Proton prefix that runs it live."""

    gaea_dir: Path
    proton: Path
    steam_root: Path
    compat_data: Path

    @classmethod
    def from_env(cls) -> "GaeaEnvironment":
        home = Path.home()
        steam = Path(os.environ.get("WGE_STEAM_ROOT", home / ".local/share/Steam"))
        return cls(
            gaea_dir=Path(os.environ.get("WGE_GAEA_DIR", home / ".local/share/gaea2")),
            proton=Path(
                os.environ.get(
                    "WGE_PROTON", steam / "steamapps/common/Proton - Experimental/proton"
                )
            ),
            steam_root=steam,
            compat_data=Path(
                os.environ.get(
                    "WGE_GAEA_COMPAT_DATA", steam / "steamapps/compatdata/gaea-swarm"
                )
            ),
        )

    @property
    def swarm(self) -> Path:
        return self.gaea_dir / "Gaea.Swarm.exe"

    @property
    def staging(self) -> Path:
        return self.gaea_dir / "wge_builds"

    def check(self) -> list[str]:
        """Every missing prerequisite, as human-readable lines. Empty means ready."""
        problems = []
        for label, path in (
            ("Gaea install", self.swarm),
            ("Proton", self.proton),
            ("Swarm prefix", self.compat_data / "pfx"),
        ):
            if not path.exists():
                problems.append(f"{label} not found at {path}")
        if shutil.which("script") is None:
            problems.append("`script` (util-linux) is not on PATH")
        return problems


@dataclass
class BuildRequest:
    graph: Path
    output_dir: Path
    resolution: int = 1024
    seed: int | None = None
    variables: dict[str, str] = field(default_factory=dict)
    ignore_cache: bool = True
    timeout_s: float = 1800.0


def windows_path(path: Path) -> str:
    """`/home/x/y` -> `Z:\\home\\x\\y`, refusing anything Swarm cannot parse."""
    absolute = str(Path(path).resolve())
    if " " in absolute:
        raise GaeaBuildError(f"path has a space, which Swarm cannot parse: {absolute}")
    converted = "Z:" + absolute.replace("/", "\\")
    if not _SAFE_TOKEN.match(converted):
        raise GaeaBuildError(f"path has characters unsafe on the Swarm command line: {absolute}")
    return converted


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def pixel_digest(path: Path) -> dict:
    """Digest of the decoded image, plus the facts a consumer needs to trust it.

    The digest covers mode, shape and raw pixel bytes, so two files with the
    same pixels and different embedded timestamps digest equal, and the same
    bytes read as a different mode do not.
    """
    with Image.open(path) as image:
        mode = image.mode
        array = np.asarray(image)
    digest = hashlib.sha256()
    digest.update(f"{mode}|{'x'.join(map(str, array.shape))}|".encode())
    digest.update(np.ascontiguousarray(array).tobytes())
    return {
        "pixel_sha256": digest.hexdigest(),
        "mode": mode,
        "shape": list(array.shape),
        "min": int(array.min()),
        "max": int(array.max()),
    }


def gaea_identity(environment: GaeaEnvironment) -> dict:
    """The Gaea and Proton builds a receipt was produced by."""
    identity: dict = {}
    swarm_dll = environment.gaea_dir / "Gaea.Swarm.dll"
    if swarm_dll.exists():
        identity["swarm_dll_sha256"] = sha256_file(swarm_dll)
    nodes_dll = environment.gaea_dir / "Gaea.Nodes.dll"
    if nodes_dll.exists():
        identity["nodes_dll_sha256"] = sha256_file(nodes_dll)
    proton_version = environment.proton.parent / "version"
    if proton_version.exists():
        identity["proton_version"] = proton_version.read_text(errors="replace").strip()
    return identity


def graph_node_types(graph_path: Path) -> list[str]:
    """Node type names in the graph, for labelling a receipt.

    Read from `$type` (`QuadSpinner.Gaea.Nodes.Erosion2, Gaea.Nodes`), not
    `Name`, which is a label the user can rename in the GUI.
    """
    import gaea_terrain  # local: keeps this module importable without the pipeline path

    graph = gaea_terrain.load_graph(graph_path)
    types = set()
    for node in gaea_terrain.nodes(graph):
        qualified = str(node.get("$type") or node.get("Name"))
        types.add(qualified.split(",")[0].rsplit(".", 1)[-1])
    return sorted(types)


def preflight(graph_path: Path) -> list[str]:
    """Why Swarm would build nothing from this graph. Empty means buildable.

    Both conditions make Swarm exit 0 in seconds having written nothing, which
    is indistinguishable from any other failure after the fact. Checking them
    here turns a silent no-op into a refusal that names the fix.
    """
    import gaea_terrain

    graph = gaea_terrain.load_graph(graph_path)
    problems = []
    if not any(node.get("SaveDefinition") for node in gaea_terrain.nodes(graph)):
        problems.append(
            "no node is marked for export (no SaveDefinition); mark one with "
            "gaea_terrain.py --export-node or the gaea_mark_export tool"
        )
    definitions = gaea_terrain.build_definitions(graph)
    if not definitions:
        problems.append("the graph has no BuildDefinition")
    elif any("Type" not in definition for definition in definitions):
        problems.append(
            "the BuildDefinition has no Type; Swarm builds nothing without one "
            "(marking an export with gaea_terrain.py fills it in as Standard)"
        )
    return problems


def swarm_command(
    environment: GaeaEnvironment, staged_graph: Path, build_path: Path, request: BuildRequest
) -> list[str]:
    """The argv for one build. Pure, so tests can assert on it."""
    if request.resolution not in RESOLUTIONS:
        raise GaeaBuildError(
            f"resolution {request.resolution} is not one Gaea builds; use one of {RESOLUTIONS}"
        )
    swarm_args = [
        "Gaea.Swarm.exe",
        windows_path(staged_graph),
        "--buildpath",
        windows_path(build_path),
        "--resolution",
        str(request.resolution),
    ]
    if request.seed is not None:
        if not -(2**31) <= int(request.seed) < 2**31:
            raise GaeaBuildError("seed must fit in an Int32")
        swarm_args += ["--seed", str(int(request.seed))]
    for name, value in sorted(request.variables.items()):
        value = str(value)
        if not _SAFE_VARIABLE.match(name) or not _SAFE_VALUE.match(value):
            raise GaeaBuildError(f"variable {name!r}={value!r} is not command-line safe")
        swarm_args += ["-v", f"{name}={value}"]
    swarm_args.append("--silent")
    if request.ignore_cache:
        swarm_args.append("--ignorecache")

    # The proton path has a space ("Proton - Experimental"), so it is
    # single-quoted for the shell `script` starts. Every Swarm token has been
    # checked against _SAFE_* above, so the inner cmd.exe string needs no quoting.
    proton = str(environment.proton).replace("'", "'\\''")
    inner = " ".join(swarm_args)
    return ["script", "-qec", f"'{proton}' run cmd.exe /c \"{inner}\"", "/dev/null"]


def _environment_variables(environment: GaeaEnvironment) -> dict[str, str]:
    variables = dict(os.environ)
    variables["STEAM_COMPAT_CLIENT_INSTALL_PATH"] = str(environment.steam_root)
    variables["STEAM_COMPAT_DATA_PATH"] = str(environment.compat_data)
    return variables


def _run(argv: list[str], cwd: Path, env: dict[str, str], timeout_s: float) -> tuple[int | None, float]:
    """Run one build. Returns (exit code or None on timeout, elapsed seconds).

    Started in its own session so a timeout can kill the whole Proton process
    tree rather than leaving a Swarm running behind a dead wrapper.
    """
    started = time.monotonic()
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        code = process.wait(timeout=timeout_s)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        return None, time.monotonic() - started
    return code, time.monotonic() - started


def build(request: BuildRequest, environment: GaeaEnvironment | None = None, runner=_run) -> dict:
    """Build `request.graph`, move its outputs to `request.output_dir`, return the receipt.

    `runner` is injectable so the staging, verification and receipt logic is
    testable without Gaea.
    """
    environment = environment or GaeaEnvironment.from_env()
    if runner is _run:
        problems = environment.check()
        if problems:
            raise GaeaBuildError("Gaea is not ready: " + "; ".join(problems))

    unbuildable = preflight(Path(request.graph))
    if unbuildable:
        raise GaeaBuildError("graph cannot build: " + "; ".join(unbuildable))

    graph_bytes = Path(request.graph).read_bytes()
    graph_sha = hashlib.sha256(graph_bytes).hexdigest()
    environment.staging.mkdir(parents=True, exist_ok=True)
    # mkdtemp's suffix is [a-z0-9_], so the staged path stays Swarm-safe, and it
    # is unique even for two builds of one graph in the same second.
    work = Path(tempfile.mkdtemp(prefix=f"{time.strftime('%Y%m%dT%H%M%S')}_{graph_sha[:12]}_", dir=environment.staging))
    try:
        build_path = work / "out"
        build_path.mkdir()
        staged = work / "graph.terrain"
        staged.write_bytes(graph_bytes)
        return _build_staged(request, environment, runner, staged, build_path, graph_sha)
    finally:
        # Staging lives on NVMe beside Gaea; failed builds would otherwise
        # accumulate there. Outputs worth keeping were already copied out.
        shutil.rmtree(work, ignore_errors=True)


def _build_staged(
    request: BuildRequest,
    environment: GaeaEnvironment,
    runner,
    staged: Path,
    build_path: Path,
    graph_sha: str,
) -> dict:
    argv = swarm_command(environment, staged, build_path, request)
    code, elapsed = runner(argv, environment.gaea_dir, _environment_variables(environment), request.timeout_s)

    produced = sorted(path for path in build_path.rglob("*") if path.is_file())
    if not produced:
        reason = "timed out" if code is None else f"exited {code}"
        raise GaeaBuildError(
            f"Swarm {reason} after {elapsed:.0f}s and wrote nothing to {build_path}. "
            "The graph passed preflight (it has an export and a build Type), so "
            "this is a Gaea-side failure; Swarm's console is not observable from "
            "here (docs/integrations/gaea-programme.md 3.3), so rebuild it at a real terminal to see why."
        )
    if code is None:
        raise GaeaBuildError(
            f"Swarm timed out after {elapsed:.0f}s with {len(produced)} partial file(s) in {build_path}"
        )

    output_dir = Path(request.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    outputs = []
    for path in produced:
        relative = path.relative_to(build_path)
        destination = output_dir / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
        entry = {"path": str(relative), "file_sha256": sha256_file(destination)}
        if destination.suffix.lower() in (".png", ".tif", ".tiff"):
            entry.update(pixel_digest(destination))
        outputs.append(entry)

    node_types = graph_node_types(staged)
    receipt = {
        "schema": RECEIPT_SCHEMA,
        "graph": {
            "source": str(Path(request.graph).resolve()),
            "sha256": graph_sha,
            "node_types": node_types,
        },
        "request": {
            "resolution": request.resolution,
            "seed": request.seed,
            "variables": dict(sorted(request.variables.items())),
            "ignore_cache": request.ignore_cache,
        },
        "gaea": gaea_identity(environment),
        "swarm_exit_code": code,
        "elapsed_s": round(elapsed, 2),
        "outputs": outputs,
        # Labelled, not measured: `compare` two receipts to measure it.
        "byte_reproducible_expected": not any(t in NONDETERMINISTIC_NODES for t in node_types),
    }
    (output_dir / "gaea-build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def compare(first_dir: Path, second_dir: Path) -> dict:
    """Pixel-level differences between two build output directories.

    Answers "is this graph reproducible, and if not, by how much", which is the
    tolerance a regenerated source artifact is held to.
    """
    first_dir, second_dir = Path(first_dir), Path(second_dir)
    names = sorted(
        {p.relative_to(first_dir) for p in first_dir.rglob("*.png")}
        | {p.relative_to(second_dir) for p in second_dir.rglob("*.png")}
    )
    rows = []
    for name in names:
        a_path, b_path = first_dir / name, second_dir / name
        if not a_path.exists() or not b_path.exists():
            rows.append({"path": str(name), "status": "missing in one build"})
            continue
        a = np.asarray(Image.open(a_path)).astype(np.int64)
        b = np.asarray(Image.open(b_path)).astype(np.int64)
        if a.shape != b.shape:
            rows.append({"path": str(name), "status": "shape differs", "shapes": [list(a.shape), list(b.shape)]})
            continue
        difference = np.abs(a - b)
        rows.append(
            {
                "path": str(name),
                "status": "identical" if not difference.any() else "differs",
                "max_abs_diff": int(difference.max()),
                "differing_fraction": round(float((difference > 0).mean()), 6),
                "p99_abs_diff": float(np.percentile(difference, 99)),
            }
        )
    return {
        "schema": COMPARE_SCHEMA,
        "first": str(first_dir),
        "second": str(second_dir),
        "pixel_identical": all(row.get("status") == "identical" for row in rows) and bool(rows),
        "outputs": rows,
    }


def _parse_variables(pairs: list[str]) -> dict[str, str]:
    variables = {}
    for pair in pairs:
        name, separator, value = pair.partition("=")
        if not separator:
            raise SystemExit(f"--var expects name=value, got {pair!r}")
        variables[name] = value
    return variables


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)

    run = commands.add_parser("build", help="build a graph and write a receipt")
    run.add_argument("graph", type=Path)
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--resolution", type=int, default=1024)
    run.add_argument("--seed", type=int)
    run.add_argument("--var", action="append", default=[], metavar="NAME=VALUE")
    run.add_argument("--use-cache", action="store_true", help="allow Gaea's build cache")
    run.add_argument("--timeout", type=float, default=1800.0)

    diff = commands.add_parser("compare", help="pixel-compare two build outputs")
    diff.add_argument("first", type=Path)
    diff.add_argument("second", type=Path)

    commands.add_parser("check", help="report missing Gaea prerequisites")

    arguments = parser.parse_args()
    if arguments.command == "check":
        problems = GaeaEnvironment.from_env().check()
        print("\n".join(problems) if problems else "ready")
        return 1 if problems else 0
    if arguments.command == "compare":
        print(json.dumps(compare(arguments.first, arguments.second), indent=2))
        return 0
    receipt = build(
        BuildRequest(
            graph=arguments.graph,
            output_dir=arguments.output,
            resolution=arguments.resolution,
            seed=arguments.seed,
            variables=_parse_variables(arguments.var),
            ignore_cache=not arguments.use_cache,
            timeout_s=arguments.timeout,
        )
    )
    print(json.dumps(receipt, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
