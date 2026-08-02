"""Generate three portable Highland hamlet/watchpost GLB variants in Blender."""

from __future__ import annotations

import math
import sys
from pathlib import Path

import bpy


def _arguments() -> Path:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise SystemExit(
            "Usage: blender --background --python "
            "generate_highland_settlement_kit.py -- OUTPUT_DIRECTORY"
        )
    return Path(args[0]).resolve()


def _material(
    name: str,
    color: tuple[float, float, float, float],
    roughness: float,
    *,
    emission: tuple[float, float, float, float] | None = None,
) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = color
    principled.inputs["Roughness"].default_value = roughness
    if emission is not None:
        emission_input = principled.inputs.get("Emission Color") or principled.inputs.get(
            "Emission"
        )
        if emission_input is not None:
            emission_input.default_value = emission
        strength = principled.inputs.get("Emission Strength")
        if strength is not None:
            strength.default_value = 1.8
    return material


def _cube(
    name: str,
    location: tuple[float, float, float],
    size: tuple[float, float, float],
    material: bpy.types.Material,
    root: bpy.types.Object,
    bevel: float = 0.0,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(
        location=(location[0], location[2], location[1])
    )
    obj = bpy.context.object
    obj.name = name
    obj.scale = (size[0] * 0.5, size[2] * 0.5, size[1] * 0.5)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel > 0.0:
        modifier = obj.modifiers.new("Weathered_edges", "BEVEL")
        modifier.width = bevel
        modifier.segments = 2
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    obj.data.materials.append(material)
    obj.parent = root
    return obj


def _cylinder(
    name: str,
    radius: float,
    height: float,
    location: tuple[float, float, float],
    material: bpy.types.Material,
    root: bpy.types.Object,
    vertices: int = 14,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices,
        radius=radius,
        depth=height,
        location=(location[0], location[2], location[1]),
    )
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(material)
    obj.parent = root
    return obj


def _roof(
    name: str,
    center: tuple[float, float, float],
    width: float,
    depth: float,
    base_y: float,
    ridge_y: float,
    material: bpy.types.Material,
    root: bpy.types.Object,
) -> bpy.types.Object:
    # Vertices are local to the house centre (not baked as absolute world
    # coordinates) and obj.location carries the offset, the same convention
    # _cube/_cylinder use via primitive_*_add(location=...). Baking world
    # coordinates into the mesh while leaving obj.location at the origin
    # made _house's later `obj.rotation_euler[2] = rotation` spin the roof
    # around the cluster's world origin instead of the house's own centre,
    # so every rotated house's roof swung away from its walls.
    x, y0, z = center
    game_vertices = [
        (-width * 0.5, base_y, -depth * 0.5),
        (width * 0.5, base_y, -depth * 0.5),
        (0.0, ridge_y, -depth * 0.5),
        (-width * 0.5, base_y, depth * 0.5),
        (width * 0.5, base_y, depth * 0.5),
        (0.0, ridge_y, depth * 0.5),
    ]
    vertices = [(vx, vz, vy) for vx, vy, vz in game_vertices]
    faces = [
        (0, 1, 2),
        (3, 5, 4),
        (0, 3, 4, 1),
        (1, 4, 5, 2),
        (2, 5, 3, 0),
    ]
    mesh = bpy.data.meshes.new(name + "_mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.location = (x, z, y0)
    obj.data.materials.append(material)
    obj.parent = root
    bevel = obj.modifiers.new("Rough_slate_edges", "BEVEL")
    bevel.width = 0.18
    bevel.segments = 2
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=bevel.name)
    return obj


def _house(
    root: bpy.types.Object,
    index: int,
    position: tuple[float, float],
    rotation: float,
    scale: float,
    materials: dict[str, bpy.types.Material],
) -> None:
    x, z = position
    width, depth = 11.0 * scale, 9.0 * scale
    foundation = _cube(
        "House_%02d_foundation" % index,
        (x, 1.0 * scale, z),
        (width + 1.5, 2.0 * scale, depth + 1.5),
        materials["dark_stone"],
        root,
        0.35,
    )
    walls = _cube(
        "House_%02d_plaster" % index,
        (x, 6.0 * scale, z),
        (width, 10.0 * scale, depth),
        materials["plaster"] if index % 2 == 0 else materials["stone"],
        root,
        0.42,
    )
    roof = _roof(
        "House_%02d_slate_roof" % index,
        (x, 0.0, z),
        width + 2.0,
        depth + 2.0,
        11.0 * scale,
        16.0 * scale,
        materials["slate"],
        root,
    )
    for obj in (foundation, walls, roof):
        obj.rotation_euler[2] = rotation
    front_x = x + math.sin(rotation) * (depth * 0.5 + 0.08)
    front_z = z + math.cos(rotation) * (depth * 0.5 + 0.08)
    door = _cube(
        "House_%02d_door" % index,
        (front_x, 4.0 * scale, front_z),
        (3.0 * scale, 7.0 * scale, 0.35),
        materials["timber"],
        root,
        0.12,
    )
    door.rotation_euler[2] = rotation
    for side in (-1.0, 1.0):
        window = _cube(
            "House_%02d_window_%d" % (index, int(side)),
            (
                front_x + math.cos(rotation) * side * width * 0.28,
                6.2 * scale,
                front_z - math.sin(rotation) * side * width * 0.28,
            ),
            (1.8 * scale, 2.1 * scale, 0.42),
            materials["window"],
            root,
            0.08,
        )
        window.rotation_euler[2] = rotation
    _cube(
        "House_%02d_chimney" % index,
        (x - width * 0.25, 15.0 * scale, z),
        (1.7 * scale, 7.0 * scale, 1.7 * scale),
        materials["dark_stone"],
        root,
        0.18,
    )


def _watchtower(
    root: bpy.types.Object,
    position: tuple[float, float],
    materials: dict[str, bpy.types.Material],
) -> None:
    x, z = position
    _cylinder("Watchtower_stone", 5.5, 19.0, (x, 9.5, z), materials["stone"], root)
    bpy.ops.mesh.primitive_cone_add(
        vertices=14,
        radius1=7.0,
        radius2=0.8,
        depth=8.0,
        location=(x, z, 23.0),
    )
    roof = bpy.context.object
    roof.name = "Watchtower_slate_roof"
    roof.data.materials.append(materials["slate"])
    roof.parent = root
    for index in range(8):
        angle = math.tau * index / 8.0
        _cube(
            "Watchtower_merlon_%02d" % index,
            (x + math.cos(angle) * 4.8, 19.5, z + math.sin(angle) * 4.8),
            (1.4, 3.0, 1.4),
            materials["dark_stone"],
            root,
            0.12,
        )


def _cluster(
    variant: int, materials: dict[str, bpy.types.Material]
) -> bpy.types.Object:
    root = bpy.data.objects.new(
        "HighlandSettlementCluster_%s" % chr(ord("A") + variant), None
    )
    bpy.context.collection.objects.link(root)
    _cylinder("Village_terrace", 35.0, 1.2, (0.0, 0.6, 0.0), materials["earth"], root, 24)
    layouts = [
        [(-18, -9, 0.45, 0.92), (-5, 15, -0.2, 1.05), (14, 12, 2.7, 0.88), (19, -11, 3.5, 1.0), (-2, -18, 0.0, 0.82)],
        [(-20, 8, 1.1, 1.0), (-12, -15, 0.35, 0.9), (9, 16, 2.8, 1.12), (18, -8, 3.7, 0.95)],
        [(-21, -5, 0.1, 0.86), (-11, 17, 1.4, 0.92), (8, 18, 2.9, 0.84), (21, 2, 3.2, 1.0), (10, -18, 4.5, 0.9), (-8, -19, 5.8, 0.82)],
    ]
    for index, (x, z, rotation, scale) in enumerate(layouts[variant]):
        _house(root, index, (x, z), rotation, scale, materials)
    _watchtower(root, (0.0, 0.0) if variant != 1 else (3.0, -1.0), materials)
    _cylinder(
        "Village_well",
        2.8,
        1.8,
        (8.0 if variant == 0 else -7.0, 0.9, -1.0),
        materials["stone"],
        root,
        16,
    )
    return root


def _select_root(root: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    root.select_set(True)
    for child in root.children_recursive:
        child.select_set(True)
    bpy.context.view_layer.objects.active = root


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    materials = {
        "stone": _material("Weathered granite", (0.12, 0.13, 0.11, 1.0), 0.94),
        "dark_stone": _material("Granite shadow", (0.035, 0.043, 0.039, 1.0), 0.98),
        "plaster": _material("Ochre limewash", (0.22, 0.18, 0.105, 1.0), 0.91),
        "slate": _material("Highland slate", (0.025, 0.038, 0.045, 1.0), 0.78),
        "timber": _material("Dark oak", (0.055, 0.027, 0.012, 1.0), 0.90),
        "earth": _material("Packed village earth", (0.11, 0.085, 0.045, 1.0), 0.97),
        "window": _material(
            "Warm window",
            (0.22, 0.11, 0.025, 1.0),
            0.55,
            emission=(0.55, 0.20, 0.035, 1.0),
        ),
    }
    for variant in range(3):
        root = _cluster(variant, materials)
        _select_root(root)
        name = "highland_settlement_cluster_%s" % chr(ord("a") + variant)
        bpy.ops.export_scene.gltf(
            filepath=str(output / (name + ".glb")),
            export_format="GLB",
            use_selection=True,
            export_materials="EXPORT",
            export_normals=True,
            export_apply=True,
        )
        if variant < 2:
            bpy.ops.object.delete(use_global=False)
    bpy.ops.wm.save_as_mainfile(filepath=str(output / "highland_settlement_kit.blend"))
    print("Generated Highland settlement kit at %s" % output)


if __name__ == "__main__":
    main()
