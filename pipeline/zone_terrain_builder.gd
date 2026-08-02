#!/usr/bin/env python3
"""One-command autonomous ZoneSpec build orchestrator.

It is intentionally the only ordered entry point a model needs to call after a
reviewed concept batch exists.  Every derived artifact is regenerated from the
canonical annotations: terrain, asset catalog/plan, all three engine adapters,
then optionally the active Godot scene, render capture, and both acceptance
reports.  It never treats a stale manifest as evidence of a new build.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path
from typing import Any, Iterable

from asset_catalog import build_catalog
from asset_plan import resolve_asset_plan
from visual_acceptance import evaluate_visual, _image
from zone_acceptance import evaluate_zone
from zone_assets_to_godot import adapt_asset_plan
from zone_compiler import ZoneCompileError, compile_file
from zone_rasterizer import rasterize_zone_spec, write_raster
from zone_to_unity import adapt_unity
from zone_to_unreal import adapt_unreal


BUILD_VERSION = "codeweald.zone-build/v1"


def _write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _run(command: list[str], project_root: Path) -> None:
    result = subprocess.run(command, cwd=project_root, text=True, capture_output=True)
    if result.stdout:
        print(result.stdout, end="")
    if result.returncode != 0:
        detail = result.stderr.strip() or "exit %d" % result.returncode
        raise ZoneCompileError("Engine command failed: %s" % detail)


def build(
    annotations: Path, project_root: Path, asset_root: Path, *, run_godot: bool = False,
    capture: bool = False, godot_bin: str = "godot",
) -> dict[str, Any]:
    project_root = project_root.resolve()
    annotations = annotations.resolve()
    if project_root not in annotations.parents:
        raise ZoneCompileError("Concept annotations must be inside project root")
    batch_dir = annotations.parent
    terrain_dir = batch_dir / "terrain"
    result = compile_file(annotations, batch_dir / "zone_spec.json", batch_dir / "validation_report.json")
    terrain = rasterize_zone_spec(result.zone_spec, source_root=batch_dir)
    write_raster(terrain, terrain_dir)
    catalog = build_catalog(project_root, asset_root)
    catalog_path = project_root / "assets/generated/codeweald_asset_catalog.json"
    _write(catalog_path, catalog)
    asset_plan = resolve_asset_plan(result.zone_spec, catalog)
    _write(batch_dir / "asset_plan.json", asset_plan)
    _write(batch_dir / "godot_asset_plan.json", adapt_asset_plan(asset_plan))
    _write(batch_dir / "unity_zone_import.json", adapt_unity(result.zone_spec, terrain.manifest, asset_plan))
    _write(batch_dir / "unreal_zone_import.json", adapt_unreal(result.zone_spec, terrain.manifest, asset_plan))

    reports: dict[str, Any] = {
        "schema_version": BUILD_VERSION,
        "zone_id": result.zone_spec["zone"]["id"],
        "stages": ["compile", "terrain", "asset_catalog", "asset_plan", "godot_adapter", "unity_adapter", "unreal_adapter"],
    }
    if run_godot:
        executable = shutil.which(godot_bin) or godot_bin
        _run([executable, "--headless", "--path", str(project_root), "--script", "res://pipeline/build_zone_spec_scene.gd"], project_root)
        reports["stages"].append("godot_scene")
        if capture:
            xvfb = shutil.which("xvfb-run")
            command = [executable, "--path", str(project_root), "--rendering-driver", "opengl3", "--script", "res://pipeline/capture_active_zone.gd"]
            _run([xvfb, "-a", *command] if xvfb else command, project_root)
            rendered = terrain_dir / "godot_active_scene.png"
            source_path = batch_dir / str(result.zone_spec["zone"]["source_images"][0]["path"])
            visual = evaluate_visual(result.zone_spec, _image(source_path), _image(rendered))
            _write(terrain_dir / "visual_acceptance_report.json", visual)
            build_report = json.loads((terrain_dir / "godot_build_report.json").read_text(encoding="utf-8"))
            semantic = evaluate_zone(result.zone_spec, terrain.manifest, build_report, visual)
            _write(terrain_dir / "acceptance_report.json", semantic)
            if semantic["status"] == "failed":
                raise ZoneCompileError("Compiled engine scene failed acceptance")
            reports["stages"].extend(["godot_capture", "visual_acceptance", "semantic_acceptance"])
    _write(batch_dir / "build_report.json", reports)
    return reports


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build every derived artifact for a reviewed Codeweald concept batch")
    parser.add_argument("annotations", type=Path)
    parser.add_argument("--project-root", type=Path, default=Path("."))
    parser.add_argument("--asset-root", type=Path, default=Path("assets"))
    parser.add_argument("--godot", action="store_true", help="replace the active Godot scene after portable artifacts build")
    parser.add_argument("--capture", action="store_true", help="capture and accept the Godot scene; implies --godot")
    parser.add_argument("--godot-bin", default="godot")
    args = parser.parse_args(argv)
    try:
        report = build(args.annotations, args.project_root, args.asset_root, run_godot=args.godot or args.capture, capture=args.capture, godot_bin=args.godot_bin)
    except (OSError, ValueError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Zone build %s: %s" % (report["zone_id"], ", ".join(report["stages"])))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
