"""Generate a portable arcane-objective ruin kit for semantic zone landmarks.

This deliberately emits a real GLB source asset rather than asking each engine
adapter to approximate a central objective with primitives or a generic fountain.
It is deterministic, has a Blender source file, and re-enters the same verified
catalog/asset-plan path as every other Codeweald prop.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import bpy


def _arguments() -> Path:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise SystemExit("Usage: blender --background --python blender_generate_arcane_ruin_kit.py -- OUTPUT_DIRECTORY")
    return Path(args[0]).resolve()


def _material(name: str, color: tuple[float, float, float], roughness: float, emission: tuple[float, float, float] | None = None, texture_path: Path | None = None) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.diffuse_color = (*color, 1.0)
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = (*color, 1.0)
    principled.inputs["Roughness"].default_value = roughness
    if texture_path and texture_path.is_file():
        texture = material.node_tree.nodes.new("ShaderNodeTexImage")
        texture.image = bpy.data.images.load(str(texture_path), check_existing=True)
        texture.interpolation = "Linear"
        material.node_tree.links.new(texture.outputs["Color"], principled.inputs["Base Color"])
    if emission:
        (principled.inputs.get("Emission Color") or principled.inputs.get("Emission")).default_value = (*emission, 1.0)
        if principled.inputs.get("Emission Strength"):
            principled.inputs["Emission Strength"].default_value = 3.5
    return material


def _cube(name: str, location: tuple[float, float, float], scale: tuple[float, float, float], material: bpy.types.Material, parent: bpy.types.Object, bevel: float = 0.0) -> bpy.types.Object:
    # Asset recipes use the shared engine convention (X/Z ground, Y up);
    # Blender authors in X/Y ground, Z up.
    bpy.ops.mesh.primitive_cube_add(location=(location[0], location[2], location[1]))
    object_ = bpy.context.object
    object_.name = name
    object_.scale = (scale[0], scale[2], scale[1])
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        modifier = object_.modifiers.new("Weathered edges", "BEVEL")
        modifier.width = bevel
        modifier.segments = 2
        bpy.context.view_layer.objects.active = object_
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    object_.data.materials.append(material)
    object_.parent = parent
    return object_


def _cylinder(name: str, radius: float, depth: float, location: tuple[float, float, float], material: bpy.types.Material, parent: bpy.types.Object, vertices: int = 12) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=(location[0], location[2], location[1]))
    object_ = bpy.context.object
    object_.name = name
    object_.data.materials.append(material)
    object_.parent = parent
    return object_


def _ruin(variant: int) -> bpy.types.Object:
    stone_texture = Path(__file__).resolve().parents[1] / "assets" / "environment" / "daoc_stone.png"
    stone = _material("Weathered rune stone", (0.10, 0.12, 0.14), 0.91, texture_path=stone_texture)
    moss = _material("Mossed lower stone", (0.075, 0.12, 0.06), 0.97)
    arcane = _material("Arcane violet crystal", (0.12, 0.02, 0.26), 0.38, (0.46, 0.05, 0.90))
    root = bpy.data.objects.new(
        "ArcaneObjectiveRuin_%s" % "AB"[variant], None
    )
    bpy.context.collection.objects.link(root)
    _cylinder("Foundation", 19.0, 1.2, (0.0, 0.6, 0.0), moss, root, 16)
    _cylinder("Rune dais", 13.5, 0.75, (0.0, 1.55, 0.0), stone, root, 16)
    _cylinder("Portal plinth", 5.5, 1.0, (0.0, 2.35, 0.0), stone, root, 10)
    # Eight deliberately uneven monoliths give a readable silhouette from the
    # map camera, while the center crystal identifies this as an objective.
    monolith_count = 8 if variant == 0 else 6
    for index in range(monolith_count):
        angle = math.tau * index / float(monolith_count)
        radius = 14.5 + (0.65 if index % 3 == 0 else -0.35)
        height = 5.5 + (index % (4 if variant == 0 else 3)) * 0.85
        pillar = _cube("RuneMonolith_%02d" % index, (math.cos(angle) * radius, 2.25 + height * 0.5, math.sin(angle) * radius), (1.35, height * 0.5, 1.05), stone if index % 2 else moss, root, 0.22)
        pillar.rotation_euler[2] = math.radians((-6.0 if index % 2 else 5.0) + index * 0.35)
        pillar.rotation_euler[1] = -angle + math.pi * 0.5
    # Pointed crystal/portal core.
    bpy.ops.mesh.primitive_cone_add(vertices=6, radius1=2.3, radius2=0.75, depth=10.0, location=(0.0, 0.0, 7.3), rotation=(0.0, 0.0, math.radians(30)))
    crystal = bpy.context.object
    crystal.name = "ArcanePortalCrystal"
    crystal.data.materials.append(arcane)
    crystal.parent = root
    if variant == 1:
        # A broken outer ring and paired focus stones distinguish the second
        # objective silhouette while retaining identical gameplay footprint.
        for side in (-1.0, 1.0):
            _cube(
                "Fallen_focus_%s" % side,
                (side * 9.0, 1.35, -8.5),
                (4.2, 1.1, 1.4),
                moss,
                root,
                0.3,
            ).rotation_euler[1] = math.radians(24.0 * side)
    return root


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    for object_ in list(bpy.data.objects):
        bpy.data.objects.remove(object_, do_unlink=True)
    for variant, suffix in enumerate(("a", "b")):
        for object_ in list(bpy.data.objects):
            bpy.data.objects.remove(object_, do_unlink=True)
        root = _ruin(variant)
        destination = output / ("arcane_objective_ruin_" + suffix)
        bpy.ops.wm.save_as_mainfile(filepath=str(destination.with_suffix(".blend")))
        bpy.ops.object.select_all(action="DESELECT")
        root.select_set(True)
        for child in root.children_recursive:
            child.select_set(True)
        bpy.context.view_layer.objects.active = root
        bpy.ops.export_scene.gltf(filepath=str(destination.with_suffix(".glb")), export_format="GLB", use_selection=True, export_materials="EXPORT", export_apply=True)
        unity_destination = output / "unity" / ("arcane_objective_ruin_" + suffix + ".fbx")
        unity_destination.parent.mkdir(parents=True, exist_ok=True)
        bpy.ops.export_scene.fbx(filepath=str(unity_destination), use_selection=True, apply_unit_scale=True, bake_space_transform=False)
        print("Generated deterministic arcane objective ruin variant at %s" % destination.with_suffix(".glb"))


if __name__ == "__main__":
    main()
