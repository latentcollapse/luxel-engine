"""Static repository fences for the native WGE path.

This is a build/orientation check, not a semantic authority. It prevents a
fresh default gate from importing archived engine adapters or silently
tracking generated output. Compatibility lanes remain available through
explicit commands and are never deleted by this module.
"""

from __future__ import annotations

import argparse
import ast
import fnmatch
import subprocess
from pathlib import Path


CANONICAL_SOURCE_ROOTS = (
    "world_core/crates",
    "graphics_lab/src",
    "terrain_lab/src",
)

CANONICAL_PYTHON_FILES = (
    "pipeline/wge_agent_surface.py",
    "pipeline/wge_engine_neutral.py",
    "pipeline/wge_native_mvp.py",
    "pipeline/wge_native_transaction.py",
    "pipeline/wge_neura_mcp.py",
    "pipeline/wge_source_intake.py",
)

ARCHIVE_PATH_MARKERS = (
    "engine_adapters",
    "godot_renderer",
    "zone_to_unity",
    "zone_to_unreal",
    "zone_spec_to_godot",
    "zone_assets_to_godot",
    "Game Projects/Codeweald",
)

TRACKED_GENERATED_PATTERNS = (
    "*/target/*",
    "target/*",
    "*/__pycache__/*",
    "__pycache__/*",
    "*.pyc",
    "*.pyo",
    ".pytest_cache/*",
    "graphify-out/*",
    "artifacts/*",
    ".freebuff/*",
    "kvfold/*",
)


def _relative_files(root: Path, directory: str, suffixes: tuple[str, ...]) -> list[Path]:
    base = root / directory
    if not base.exists():
        return []
    return sorted(
        path
        for path in base.rglob("*")
        if path.is_file() and path.suffix in suffixes
    )


def _canonical_source_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for directory in CANONICAL_SOURCE_ROOTS:
        files.extend(_relative_files(root, directory, (".rs", ".jl")))
    files.extend(root / relative for relative in CANONICAL_PYTHON_FILES if (root / relative).is_file())
    return files


def _canonical_import_violations(root: Path) -> list[str]:
    violations: list[str] = []
    forbidden_modules = ("engine_adapters", "godot", "unity", "unreal", "zone_to_")

    for path in _canonical_source_files(root):
        relative = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8", errors="replace")
        for marker in ARCHIVE_PATH_MARKERS:
            if marker in text:
                violations.append(f"{relative}: archived path marker {marker!r}")

        if path.suffix != ".py":
            continue
        try:
            tree = ast.parse(text, filename=relative)
        except SyntaxError as exc:
            violations.append(f"{relative}: syntax error in canonical Python: {exc}")
            continue
        for node in ast.walk(tree):
            imported: str | None = None
            if isinstance(node, ast.Import):
                imported = node.names[0].name if node.names else None
            elif isinstance(node, ast.ImportFrom):
                imported = node.module
            if imported and any(
                imported == marker or imported.startswith(f"{marker}.")
                for marker in forbidden_modules
            ):
                violations.append(f"{relative}: archived import {imported!r}")
    return violations


def _tracked_files(root: Path) -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        check=True,
        capture_output=True,
    )
    return [item for item in result.stdout.decode().split("\0") if item]


def _tracked_generated_violations(root: Path) -> list[str]:
    violations: list[str] = []
    for relative in _tracked_files(root):
        if any(fnmatch.fnmatch(relative, pattern) for pattern in TRACKED_GENERATED_PATTERNS):
            violations.append(f"tracked generated/cache path: {relative}")
    return violations


def violations(root: Path) -> list[str]:
    """Return all native-path fence violations, without mutating the tree."""

    try:
        return _canonical_import_violations(root) + _tracked_generated_violations(root)
    except (OSError, subprocess.CalledProcessError) as exc:
        return [f"repository fence infrastructure failure: {exc}"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="WGE repository root (default: inferred from this file)",
    )
    args = parser.parse_args()
    root = args.root.resolve()
    failures = violations(root)
    if failures:
        print("WGE native repository fences: FAILED")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("WGE native repository fences: PASS")
    print(f"- canonical source roots checked: {len(CANONICAL_SOURCE_ROOTS)}")
    print(f"- canonical Python modules checked: {len(CANONICAL_PYTHON_FILES)}")
    print("- tracked generated/cache paths: none")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
