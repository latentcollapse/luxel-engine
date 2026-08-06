#!/usr/bin/env python3
"""Build the Bevy world viewer with a self-reported source-tree digest.

The resulting binary can report what sources it was actually built from via
`--provenance`, so capture_bevy.py can refuse to trust a binary that predates
the sources it's about to be measured against -- tooling item 1. A binary
built with plain `cargo build`/`cargo run` has no digest to report and prints
"unknown", which capture_bevy.py also refuses: unknown provenance is not
trustworthy provenance.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

from tool_provenance import source_tree_digest

# One level up, not two: the reference renderer ships with the engine. Two
# pointed above the engine entirely once WGE moved out of Codeweald (D8) --
# the fourth and last instance of that assumption.
WORLD_CORE = Path(__file__).resolve().parents[1] / "world_core"
VIEWER_SOURCE_DIRECTORIES = ("apps", "crates")


def compute_digest() -> str:
    roots = [WORLD_CORE / directory for directory in VIEWER_SOURCE_DIRECTORIES]
    return source_tree_digest(roots, extensions=(".rs",))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--profile",
        choices=("debug", "release"),
        default="release",
        help="cargo build profile (default: release, matching the codeweald launcher)",
    )
    arguments = parser.parse_args(argv)

    digest = compute_digest()
    cmd = ["cargo", "build", "-p", "codeweald-world-viewer"]
    if arguments.profile == "release":
        cmd.append("--release")
    env = {**os.environ, "CODEWEALD_SOURCE_DIGEST": digest}
    print(f"Source digest: {digest}")
    print(f"Command: {' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=WORLD_CORE, env=env)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
