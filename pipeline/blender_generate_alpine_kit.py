"""Generate deterministic Alpine massif GLBs for Codeweald asset profiles.

Run with Blender in background mode.  These are not generic cones: each mesh is a
flat-shaded ridge with an irregular footprint, several arêtes, rocky lower faces,
and snow-bearing high faces.  The resulting source assets re-enter the regular
catalog/asset-plan path and therefore remain auditable and engine-portable.
"""

from __future__ import annotations

import math
import random
import sys
from pathlib import Path

import bpy


def _arguments() -> Path:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise SystemExit("Usage: blender --background --python blender_generate_alpine_kit.py -- OUTPUT_DIRECTORY")
    return Path(args[0]).resolve()


def _material(name: str, color: tuple[float, float, float], roughness: float) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.diffuse_color = (*color, 1.0)
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = (*color, 1.0)
    principled.inputs["Roughness"].default_value = roughness
    principled.inputs["Metallic"].default_value = 0.0
    return material


def _ridge_mesh(name: str, seed: int, width: float, depth: float, peak: float) -> bpy.types.Object:
    rng = random.Random(seed)
    vertices: list[tuple[float, float, float]] = []
    faces: list[tuple[int, int, int]] = []
    # A chain of overlapping asymmetric rock spires creates the silhouette a
    # heightfield cannot: separate summits, narrow saddles, and hard facets.
    # The deterministic seed makes it an auditable asset, not a random editor
    # placement that can change between engines.
    spire_count = 7
    for spire in range(spire_count):
        progress = spire / float(spire_count - 1)
        center_x = (progress - 0.5) * width * 0.72 + rng.uniform(-10.0, 10.0)
        center_z = math.sin(progress * math.pi * 1.65 + seed * 0.03) * depth * 0.19 + rng.uniform(-16.0, 16.0)
        radius_x = rng.uniform(width * 0.075, width * 0.14)
        radius_z = rng.uniform(depth * 0.10, depth * 0.19)
        height = peak * rng.uniform(0.56, 1.0)
        sides = 7
        base_index = len(vertices)
        for ring_height, radius_factor in ((0.0, 1.0), (height * 0.46, 0.62)):
            for side in range(sides):
                angle = (side / float(sides)) * math.tau + rng.uniform(-0.10, 0.10)
                vertices.append((
                    center_x + math.cos(angle) * radius_x * radius_factor * rng.uniform(0.82, 1.16),
                    ring_height,
                    center_z + math.sin(angle) * radius_z * radius_factor * rng.uniform(0.82, 1.16),
                ))
        apex_index = len(vertices)
        vertices.append((center_x + rng.uniform(-radius_x * 0.18, radius_x * 0.18), height, center_z + rng.uniform(-radius_z * 0.18, radius_z * 0.18)))
        for side in range(sides):
            next_side = (side + 1) % sides
            lower_a, lower_b = base_index + side, base_index + next_side
            upper_a, upper_b = base_index + sides + side, base_index + sides + next_side
            faces.extend(((lower_a, lower_b, upper_b), (lower_a, upper_b, upper_a), (upper_a, upper_b, apex_index)))
    mesh = bpy.data.meshes.new(name + "_mesh")
    mesh.from_pydata(vertices, [], faces)
    # Keep the kit grounded in the painted, overcast map palette.  Snow is a
    # sparse high-altitude accent, not a glossy white blanket when seen from
    # the strategic overview camera.
    mesh.materials.append(_material("Highland granite", (0.09, 0.11, 0.12), 0.93))
    mesh.materials.append(_material("Alpine snow", (0.34, 0.38, 0.40), 0.88))
    mesh.materials.append(_material("Mossy talus", (0.10, 0.14, 0.08), 0.98))
    mesh.update()
    snowline = peak * 0.83
    for polygon in mesh.polygons:
        average_height = sum(mesh.vertices[index].co.y for index in polygon.vertices) / len(polygon.vertices)
        polygon.use_smooth = False
        polygon.material_index = 1 if average_height >= snowline else (2 if average_height < peak * 0.16 else 0)
    object_ = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(object_)
    return object_


def _export(object_: bpy.types.Object, destination: Path) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    object_.select_set(True)
    bpy.context.view_layer.objects.active = object_
    bpy.ops.wm.save_as_mainfile(filepath=str(destination.with_suffix(".blend")))
    bpy.ops.export_scene.gltf(
        filepath=str(destination.with_suffix(".glb")),
        export_format="GLB",
        use_selection=True,
        export_materials="EXPORT",
        export_apply=True,
    )
    unity_destination = destination.parent / "unity" / (destination.name + ".fbx")
    unity_destination.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.fbx(filepath=str(unity_destination), use_selection=True, apply_unit_scale=True, bake_space_transform=False)
    bpy.data.objects.remove(object_, do_unlink=True)


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    for leftover in list(bpy.data.objects):
        bpy.data.objects.remove(leftover, do_unlink=True)
    variants = [
        ("alpine_cliff_ridge_a", 71, 250.0, 165.0, 185.0),
        ("alpine_cliff_ridge_b", 193, 220.0, 195.0, 175.0),
        ("alpine_cliff_ridge_c", 311, 280.0, 150.0, 205.0),
        ("alpine_cliff_ridge_d", 487, 190.0, 180.0, 160.0),
        ("alpine_cliff_ridge_e", 617, 245.0, 170.0, 195.0),
        ("alpine_cliff_ridge_f", 829, 215.0, 210.0, 180.0),
    ]
    for name, seed, width, depth, peak in variants:
        _export(_ridge_mesh(name, seed, width, depth, peak), output / name)
    print("Generated %d deterministic Alpine massif assets in %s" % (len(variants), output))


if __name__ == "__main__":
    main()
