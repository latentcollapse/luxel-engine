"""Headless Blender worker for Codeweald asset visual preflight.

This file is deliberately executable by Blender, not imported by the normal
Python pipeline.  Given the already resolved asset plan it imports every
selected model, records real mesh/material/bounds facts, and renders a compact
workbench thumbnail.  It never repairs or substitutes a model: an import error
is reported back to the portable pipeline as a failure.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector


def _arguments() -> tuple[Path, Path, Path]:
    if "--" not in sys.argv:
        raise RuntimeError("Expected -- <project-root> <asset-plan> <output-dir>")
    values = sys.argv[sys.argv.index("--") + 1 :]
    if len(values) != 3:
        raise RuntimeError("Expected -- <project-root> <asset-plan> <output-dir>")
    return Path(values[0]).resolve(), Path(values[1]).resolve(), Path(values[2]).resolve()


def _selected_assets(plan: dict) -> list[dict]:
    assets: dict[str, dict] = {}
    for assignment in plan.get("assignments", []):
        if not isinstance(assignment, dict):
            continue
        layers = assignment.get("layers", []) or [assignment]
        for layer in layers:
            if not isinstance(layer, dict):
                continue
            for asset in layer.get("assets", []):
                if isinstance(asset, dict) and isinstance(asset.get("source_path"), str):
                    path = asset["source_path"]
                    if path not in assets:
                        assets[path] = dict(asset)
                        assets[path]["quality_requirements"] = {}
                        assets[path]["appearance_requirements"] = {}
                    requirements = layer.get("quality_requirements", {})
                    if isinstance(requirements, dict):
                        for key in ("minimum_triangle_count", "minimum_material_slots"):
                            if key in requirements:
                                assets[path]["quality_requirements"][key] = max(int(requirements[key]), int(assets[path]["quality_requirements"].get(key, 0)))
                    appearance = layer.get("appearance_requirements", {})
                    if isinstance(appearance, dict):
                        assets[path]["appearance_requirements"].update(appearance)
    return [assets[path] for path in sorted(assets)]


def _clean() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablock in list(bpy.data.materials):
        bpy.data.materials.remove(datablock)


def _import(path: Path) -> None:
    suffix = path.suffix.lower()
    if suffix in {".glb", ".gltf"}:
        bpy.ops.import_scene.gltf(filepath=str(path))
    elif suffix == ".fbx":
        bpy.ops.import_scene.fbx(filepath=str(path))
    elif suffix == ".obj":
        bpy.ops.wm.obj_import(filepath=str(path))
    else:
        raise RuntimeError("Unsupported model format %s" % suffix)


def _mesh_objects() -> list:
    return [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]


def _bounds(objects: list) -> tuple[Vector, Vector]:
    points: list[Vector] = []
    for obj in objects:
        points.extend(obj.matrix_world @ Vector(corner) for corner in obj.bound_box)
    return Vector((min(point.x for point in points), min(point.y for point in points), min(point.z for point in points))), Vector((max(point.x for point in points), max(point.y for point in points), max(point.z for point in points)))


def _camera_for(bounds_min: Vector, bounds_max: Vector) -> None:
    center = (bounds_min + bounds_max) * 0.5
    extent = max((bounds_max - bounds_min).length, 0.25)
    camera_data = bpy.data.cameras.new("CodewealdPreflightCamera")
    camera = bpy.data.objects.new("CodewealdPreflightCamera", camera_data)
    bpy.context.scene.collection.objects.link(camera)
    camera.location = center + Vector((extent * 1.45, -extent * 1.45, extent * 0.95))
    direction = center - camera.location
    camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    camera_data.type = "ORTHO"
    camera_data.ortho_scale = extent * 1.7
    bpy.context.scene.camera = camera


def _thumbnail(path: Path, bounds_min: Vector, bounds_max: Vector) -> None:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.studio_light = "paint.sl"
    # TEXTURE reveals whether the portable import retained its image-backed
    # PBR base color. MATERIAL only shows Blender's viewport color and allowed
    # a white GLB with a nominal material slot to pass unnoticed.
    scene.display.shading.color_type = "TEXTURE"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.display.shading.cavity_type = "WORLD"
    scene.render.resolution_x = 256
    scene.render.resolution_y = 256
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.film_transparent = False
    scene.world.color = (0.045, 0.055, 0.075)
    _camera_for(bounds_min, bounds_max)
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def _probe(project_root: Path, asset: dict, output_dir: Path) -> dict:
    relative = str(asset["source_path"])
    source = (project_root / relative).resolve()
    entry = {"source_path": relative, "asset_id": asset.get("id"), "thumbnail": None, "status": "failed", "failures": []}
    try:
        if project_root not in source.parents or not source.is_file():
            raise RuntimeError("model path is unavailable inside project root")
        _clean()
        _import(source)
        objects = _mesh_objects()
        if not objects:
            raise RuntimeError("import contains no mesh geometry")
        bounds_min, bounds_max = _bounds(objects)
        dimensions = bounds_max - bounds_min
        triangles = sum(len(mesh.data.polygons) for mesh in objects)
        material_slots = sum(len(mesh.data.materials) for mesh in objects)
        if triangles <= 0:
            raise RuntimeError("import contains no renderable polygons")
        stem = source.as_posix().replace("/", "_").replace(".", "_")
        thumbnail = output_dir / (stem + ".png")
        _thumbnail(thumbnail, bounds_min, bounds_max)
        requirements = asset.get("quality_requirements", {})
        entry.update({
            "status": "passed",
            "thumbnail": thumbnail.name,
            "mesh_object_count": len(objects),
            "triangle_count": triangles,
            "material_slot_count": material_slots,
            "bounds_m": {"min": [round(value, 5) for value in bounds_min], "max": [round(value, 5) for value in bounds_max], "size": [round(value, 5) for value in dimensions]},
            "quality_requirements": requirements,
            "appearance_requirements": asset.get("appearance_requirements", {}),
        })
        if triangles < int(requirements.get("minimum_triangle_count", 0)):
            entry["failures"].append("triangle count %d is below required %d" % (triangles, int(requirements["minimum_triangle_count"])))
        if material_slots < int(requirements.get("minimum_material_slots", 0)):
            entry["failures"].append("material slot count %d is below required %d" % (material_slots, int(requirements["minimum_material_slots"])))
        if entry["failures"]:
            entry["status"] = "failed"
    except Exception as exc:  # Blender exposes varied importer exception types.
        entry["failures"].append(str(exc))
    return entry


def main() -> None:
    project_root, plan_path, output_dir = _arguments()
    plan = json.loads(plan_path.read_text(encoding="utf-8"))
    output_dir.mkdir(parents=True, exist_ok=True)
    entries = [_probe(project_root, asset, output_dir) for asset in _selected_assets(plan)]
    report = {
        "schema_version": "codeweald.blender-asset-probe/v1",
        "zone_id": plan.get("zone_id"),
        "assets": entries,
        "status": "passed" if entries and all(entry["status"] == "passed" for entry in entries) else "failed",
    }
    (output_dir / "probe_report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
