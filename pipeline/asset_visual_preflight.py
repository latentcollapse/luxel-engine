#!/usr/bin/env python3
"""Run Blender asset preflight and compose a reviewable selected-asset sheet.

The ZoneSpec describes what a feature needs. The asset plan resolves concrete
files. This stage proves those concrete files actually import as meshes and
gives a human or vision model an auditable image sheet of exactly what was
selected, before an engine scene is allowed to claim visual fidelity.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path
from statistics import fmean
from typing import Any, Iterable

from PIL import Image, ImageDraw, ImageFont

from asset_plan import ASSET_PLAN_VERSION
from zone_compiler import ZoneCompileError


PREFLIGHT_VERSION = "codeweald.asset-visual-preflight/v1"
_PROBE = Path(__file__).with_name("blender_asset_probe.py")


def _appearance_metrics(image: Image.Image) -> dict[str, float]:
    """Measure the rendered asset while excluding the fixed studio backdrop."""
    sample = image.convert("RGB").resize((128, 128), Image.Resampling.LANCZOS)
    pixels = list(sample.get_flattened_data())
    width, height = sample.size
    corners = (0, width - 1, (height - 1) * width, height * width - 1)
    background = tuple(fmean(pixels[index][channel] for index in corners) for channel in range(3))
    foreground = [
        pixel for pixel in pixels
        if sum(abs(float(pixel[channel]) - background[channel]) for channel in range(3)) / (3.0 * 255.0) >= 0.045
    ]
    if not foreground:
        return {"foreground_fraction": 0.0, "mean_luminance": 0.0, "bright_fraction": 0.0}
    luminance = [
        (0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]) / 255.0
        for pixel in foreground
    ]
    return {
        "foreground_fraction": round(len(foreground) / len(pixels), 6),
        "mean_luminance": round(fmean(luminance), 6),
        "bright_fraction": round(sum(value >= 0.68 for value in luminance) / len(luminance), 6),
    }


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _run_blender(project_root: Path, asset_plan: Path, output_dir: Path, blender_bin: str) -> dict[str, Any]:
    executable = shutil.which(blender_bin) or blender_bin
    command = [executable, "--background", "--python", str(_PROBE), "--", str(project_root), str(asset_plan), str(output_dir)]
    result = subprocess.run(command, text=True, capture_output=True)
    report_path = output_dir / "probe_report.json"
    if not report_path.is_file():
        detail = result.stderr.strip() or result.stdout.strip() or "Blender exited %d" % result.returncode
        raise ZoneCompileError("Blender asset probe did not write a report: %s" % detail)
    return _read(report_path)


def _contact_sheet(entries: list[dict[str, Any]], thumbnail_dir: Path, output: Path) -> dict[str, int]:
    tiles: list[tuple[Image.Image, str, str]] = []
    for entry in entries:
        thumbnail = entry.get("thumbnail")
        if entry.get("status") != "passed" or not isinstance(thumbnail, str):
            continue
        try:
            image = Image.open(thumbnail_dir / thumbnail).convert("RGB")
        except OSError:
            continue
        tiles.append((image, str(entry.get("source_path", "asset")), "%d tris | %d mats" % (int(entry.get("triangle_count", 0)), int(entry.get("material_slot_count", 0)))))
    columns, tile_size, label_height = 4, 256, 34
    rows = max(1, (len(tiles) + columns - 1) // columns)
    sheet = Image.new("RGB", (columns * tile_size, rows * (tile_size + label_height)), (16, 19, 25))
    draw = ImageDraw.Draw(sheet)
    font = ImageFont.load_default()
    for index, (image, label, metadata) in enumerate(tiles):
        x, y = (index % columns) * tile_size, (index // columns) * (tile_size + label_height)
        sheet.paste(image.resize((tile_size, tile_size), Image.Resampling.LANCZOS), (x, y))
        draw.text((x + 4, y + tile_size + 3), label[-42:], fill=(230, 235, 240), font=font)
        draw.text((x + 4, y + tile_size + 17), metadata, fill=(145, 180, 210), font=font)
    output.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(output)
    return {"rendered_tiles": len(tiles), "columns": columns, "rows": rows}


def preflight(asset_plan: Path, project_root: Path, output_dir: Path, *, blender_bin: str = "blender") -> dict[str, Any]:
    project_root, asset_plan, output_dir = project_root.resolve(), asset_plan.resolve(), output_dir.resolve()
    # The plan *file* may live anywhere -- it is an artifact of the batch, and a
    # batch need not sit inside the engine (D8). What has to resolve under the
    # engine root is the assets the plan names, and the probe is handed
    # `project_root` separately for exactly that. Requiring the file itself to
    # be in-tree constrained the wrong thing.
    if not project_root.is_dir():
        raise ZoneCompileError("Asset root %s is not a directory" % project_root)
    plan = _read(asset_plan)
    if plan.get("schema_version") != ASSET_PLAN_VERSION:
        raise ZoneCompileError("Expected %s" % ASSET_PLAN_VERSION)
    probe = _run_blender(project_root, asset_plan, output_dir, blender_bin)
    entries = probe.get("assets", []) if isinstance(probe.get("assets"), list) else []
    failures: list[str] = []
    if not entries:
        failures.append("asset plan selected no inspectable assets")
    for entry in entries:
        if not isinstance(entry, dict) or entry.get("status") != "passed":
            failures.append("asset probe failed: %s" % (entry.get("source_path", "unknown") if isinstance(entry, dict) else "invalid entry"))
            continue
        if int(entry.get("mesh_object_count", 0)) <= 0 or int(entry.get("triangle_count", 0)) <= 0:
            failures.append("asset probe found no renderable geometry: %s" % entry.get("source_path", "unknown"))
        size = entry.get("bounds_m", {}).get("size", []) if isinstance(entry.get("bounds_m"), dict) else []
        if not isinstance(size, list) or len(size) != 3 or max(float(value) for value in size) <= 0.001:
            failures.append("asset probe found degenerate bounds: %s" % entry.get("source_path", "unknown"))
        thumbnail = entry.get("thumbnail")
        if isinstance(thumbnail, str):
            try:
                metrics = _appearance_metrics(Image.open(output_dir / thumbnail))
            except OSError:
                failures.append("asset probe thumbnail is unreadable: %s" % entry.get("source_path", "unknown"))
            else:
                entry["appearance_metrics"] = metrics
                requirements = entry.get("appearance_requirements", {})
                if isinstance(requirements, dict):
                    maximum_luminance = requirements.get("maximum_mean_luminance")
                    if maximum_luminance is not None and metrics["mean_luminance"] > float(maximum_luminance):
                        failures.append(
                            "asset appearance is too bright for %s: %.3f > %.3f"
                            % (entry.get("source_path", "unknown"), metrics["mean_luminance"], float(maximum_luminance))
                        )
                    maximum_bright = requirements.get("maximum_bright_fraction")
                    if maximum_bright is not None and metrics["bright_fraction"] > float(maximum_bright):
                        failures.append(
                            "asset appearance has too many bright pixels for %s: %.3f > %.3f"
                            % (entry.get("source_path", "unknown"), metrics["bright_fraction"], float(maximum_bright))
                        )
    sheet = _contact_sheet(entries, output_dir, output_dir / "selected_assets_contact_sheet.png")
    report = {
        "schema_version": PREFLIGHT_VERSION,
        "zone_id": plan.get("zone_id"),
        # Named relative to its own batch, not to the engine. This report is a
        # batch artifact and its bytes are hashed into `render_plan.json`, so an
        # engine-relative or absolute name here would fold the batch's location
        # on disk into every downstream hash -- the same world compiled in two
        # places would stop being the same world (D22). A sibling file named by
        # basename is unambiguous from inside the batch and identical wherever
        # the batch is kept.
        "asset_plan": asset_plan.relative_to(asset_plan.parent).as_posix(),
        "status": "failed" if failures else "passed",
        "failures": failures,
        "asset_count": len(entries),
        "contact_sheet": "selected_assets_contact_sheet.png",
        "contact_sheet_layout": sheet,
        "assets": entries,
    }
    (output_dir / "asset_visual_preflight_report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if failures:
        raise ZoneCompileError("Asset visual preflight failed: %s" % "; ".join(failures))
    return report


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Probe selected ZoneSpec assets in Blender and write visual evidence")
    parser.add_argument("asset_plan", type=Path)
    parser.add_argument("--project-root", type=Path, default=Path("."))
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--blender-bin", default="blender")
    args = parser.parse_args(argv)
    try:
        report = preflight(args.asset_plan, args.project_root, args.output_dir, blender_bin=args.blender_bin)
    except (OSError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Asset visual preflight %s: %s (%d assets)" % (report["zone_id"], report["status"], report["asset_count"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
