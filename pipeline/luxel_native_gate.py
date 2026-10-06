"""Run the bounded default native Luxel gate.

The broad historical Python discovery suite and cold GPU/Lava integration
tests are intentionally not the default: they contain provider/compatibility
lanes or take minutes while compiling and initializing a worker. Those lanes
remain available through ``--gpu`` or explicit commands. This entry point is
the reproducible native gate a fresh agent should run first.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

from luxel_repository_fences import violations


NATIVE_PYTHON_TESTS = (
    "tests.test_luxel_control_plane",
    "tests.test_luxel_construction_smoke",
    "tests.test_luxel_repository_fences",
)

NATIVE_RUST_PACKAGES = (
    "luxel-asset-contract",
    "luxel-gameplay-contract",
    "luxel-project-ledger",
    "luxel-reference-runtime",
    "luxel-intake-repair-contract",
    "luxel-certification-authority",
    "luxel-live-evidence-contract",
    "luxel-control-plane",
)


def _run(command: list[str], root: Path) -> int:
    print("+", " ".join(command), flush=True)
    return subprocess.run(command, cwd=root, check=False).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-rust", action="store_true")
    parser.add_argument("--skip-python", action="store_true")
    parser.add_argument("--fences-only", action="store_true")
    parser.add_argument(
        "--gpu",
        action="store_true",
        help="also run cold Julia/Lava worker, native graphics, and visual smoke tests",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    failures = violations(root)
    if failures:
        print("native repository fences failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("+ repository fences")
    if args.fences_only:
        return 0

    if not args.skip_rust:
        rust_commands = (
            ["cargo", "fmt", "--manifest-path", "world_core/Cargo.toml", "--all", "--", "--check"],
            ["cargo", "check", "--manifest-path", "world_core/Cargo.toml", "--offline"],
            ["cargo", "test", "--manifest-path", "world_core/Cargo.toml", "--offline", "--lib"],
        )
        for command in rust_commands:
            if _run(command, root):
                return 1
        for package in NATIVE_RUST_PACKAGES:
            if _run(
                [
                    "cargo",
                    "test",
                    "--manifest-path",
                    "world_core/Cargo.toml",
                    "--offline",
                    "--package",
                    package,
                ],
                root,
            ):
                return 1
        if _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
                "--test",
                "visual_quality",
            ],
            root,
        ):
            return 1
        if _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
                "--test",
                "asset_projection",
            ],
            root,
        ):
            return 1
        if _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
                "--test",
                "scene_composition",
            ],
            root,
        ):
            return 1
        if _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
                "--test",
                "real_asset_composition",
            ],
            root,
        ):
            return 1
        if args.gpu and _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
            ],
            root,
        ):
            return 1
        if args.gpu and _run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "world_core/Cargo.toml",
                "--offline",
                "--package",
                "luxel-native-graphics-contract",
                "--test",
                "real_asset_composition",
                "--",
                "--ignored",
            ],
            root,
        ):
            return 1
        if args.gpu and _run(
            [
                os.environ.get("LUXEL_JULIA", "julia"),
                "--project=graphics_lab",
                "graphics_lab/test/runtests.jl",
            ],
            root,
        ):
            return 1

    if not args.skip_python:
        python_tests = list(NATIVE_PYTHON_TESTS)
        if args.gpu:
            python_tests.extend(("tests.test_luxel_engine_neutral", "tests.test_luxel_native_mvp"))
        if _run([sys.executable, "-m", "unittest", *python_tests], root):
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
