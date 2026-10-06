#!/usr/bin/env python3
"""Run the lane-overlap semantic transaction and print the kernel transcript.

Authoring files are parsed as data. Commit, the predicate, and receipt
minting happen in the Rust kernel. Julia only measures overlap.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

from luxel_semantic_ir import canonical_dumps, lower_file

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "world_core" / "crates" / "semantic_kernel" / "registry_v0.json"
WORKER = ROOT / "terrain_lab" / "bin" / "lane_overlap_worker.jl"
PROJECT = ROOT / "terrain_lab"
MANIFEST = PROJECT / "Manifest.toml"
FIXTURES = ROOT / "tests" / "fixtures" / "semantic_kernel"
BINARY = ROOT / "world_core" / "target" / "debug" / "luxel-semantic-kernel"


def build_binary() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "luxel-semantic-kernel", "--quiet"],
        cwd=ROOT / "world_core",
        check=True,
    )
    if not BINARY.is_file():
        raise SystemExit(f"missing kernel binary: {BINARY}")
    return BINARY


def write_ir(source: Path, directory: Path, name: str) -> Path:
    document = lower_file(source, REGISTRY)
    path = directory / name
    path.write_text(canonical_dumps(document) + "\n", encoding="utf-8")
    return path


def run_demo(store: Path, *, solver_image: str | None = None) -> int:
    binary = build_binary()
    invalid = FIXTURES / "invalid.luxel"
    repaired = FIXTURES / "repaired.luxel"
    tampered = FIXTURES / "tampered.luxel"
    command = [
        str(binary),
        "run",
        "--store",
        str(store),
        "--invalid-source",
        str(invalid),
        "--invalid-ir",
        str(write_ir(invalid, store, "invalid.ir.json")),
        "--repaired-source",
        str(repaired),
        "--repaired-ir",
        str(write_ir(repaired, store, "repaired.ir.json")),
        "--tampered-source",
        str(tampered),
        "--tampered-ir",
        str(write_ir(tampered, store, "tampered.ir.json")),
        "--registry",
        str(REGISTRY),
        "--worker",
        str(WORKER),
        "--project",
        str(PROJECT),
        "--manifest",
        str(MANIFEST),
    ]
    if solver_image is not None:
        command.extend(["--solver-image", solver_image])
    completed = subprocess.run(command, text=True)
    return completed.returncode


def main() -> None:
    parser = argparse.ArgumentParser(description="Demonstrate the Luxel semantic transaction kernel")
    parser.add_argument("--store", type=Path)
    args = parser.parse_args()
    if args.store is not None:
        args.store.mkdir(parents=True, exist_ok=True)
        code = run_demo(args.store)
    else:
        with tempfile.TemporaryDirectory(prefix="luxel-kernel-demo-") as directory:
            code = run_demo(Path(directory))
    sys.exit(code)


if __name__ == "__main__":
    main()
