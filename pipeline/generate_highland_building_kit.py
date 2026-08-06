"""Generate individual Highland building GLBs: cottages, a well, a watchtower.

`generate_highland_settlement_kit.py` bakes five houses, a well, a terrace and
a watchtower into one glb per village. That meant a village was placed as a
single instance -- buildings could not be individually positioned, rotated or
removed -- and only three variants existed, so every one of the ~10 villages
on the map looked identical. This kit emits **one glb per building** so a
placement solver can compose varied villages from a palette instead of
stamping identical clusters.

Cotswold-style medieval cottages (stone/plaster walls, steep slate or thatch
roofs, small windows) are the DAoC-inspired look Matt asked for. Log cabins
are a later pass, deliberately not built here.

Every mesh node is named `<Building>_<part>` so `asset_parts.py` groups
collision by prefix (`Cottage_A_slate_roof`, `Cottage_A_door`, ...). The roof
mesh is built with `highland_building_geometry.roof_local_vertices`, which
keeps the same local-vertex / `obj.location`-carries-the-offset convention
`generate_highland_settlement_kit._roof` was fixed to use -- see that
function's docstring and `highland_building_geometry`'s module docstring for
the historical bug this avoids.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))

from highland_building_geometry import (  # noqa: E402
    COTTAGE_VARIANTS,
    CottageSpec,
    roof_local_vertices,
)


def _arguments() -> Path:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise SystemExit(
            "Usage: blender --background --python "
            "generate_highland_building_kit.py -- OUTPUT_DIRECTORY"
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
    bpy.ops.mesh.primitive_cube_add(location=(location[0], location[2], location[1]))
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
        vertices=vertices, radius=radius, depth=height, location=(location[0], location[2], location[1])
    )
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(material)
    obj.parent = root
    return obj


def _cone(
    name: str,
    radius: float,
    height: float,
    location: tuple[float, float, float],
    material: bpy.types.Material,
    root: bpy.types.Object,
    vertices: int = 14,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cone_add(
        vertices=vertices, radius1=radius, radius2=0.0, depth=height, location=(location[0], location[2], location[1])
    )
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(material)
    obj.parent = root
    return obj


def _roof(
    name: str,
    width: float,
    depth: float,
    base_height: float,
    ridge_height: float,
    material: bpy.types.Material,
    root: bpy.types.Object,
) -> bpy.types.Object:
    # Vertices come from `roof_local_vertices`, which is local to the
    # building's own origin (not baked as absolute world coordinates); the
    # world offset (there is none here -- every building is generated at its
    # own origin) is carried by `obj.location`, never folded into the mesh.
    # See `highland_building_geometry`'s module docstring for the bug this
    # avoids: baking world coordinates into the mesh while leaving
    # `obj.location` at the origin makes a later rotation spin the roof
    # around the wrong pivot and swing it away from the walls.
    game_vertices = roof_local_vertices(width, depth, base_height, ridge_height)
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
    obj.location = (0.0, 0.0, 0.0)
    obj.data.materials.append(material)
    obj.parent = root
    bevel = obj.modifiers.new("Rough_roof_edges", "BEVEL")
    bevel.width = 0.12
    bevel.segments = 2
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=bevel.name)
    return obj


_DOOR_LOCAL_POSITIONS = {
    # (wall_axis, sign, along_fraction): which wall the door sits on, which
    # side of it faces outward, and how far along that wall it is offset.
    "front_left": ("depth", 1.0, -0.28),
    "front_center": ("depth", 1.0, 0.0),
    "front_right": ("depth", 1.0, 0.28),
    "side": ("width", 1.0, 0.1),
}


def _cottage(spec: CottageSpec, materials: dict[str, bpy.types.Material]) -> bpy.types.Object:
    root = bpy.data.objects.new("Cottage_%s" % spec.key, None)
    bpy.context.collection.objects.link(root)

    foundation_height = 0.5
    _cube(
        "Cottage_%s_foundation" % spec.key,
        (0.0, foundation_height * 0.5, 0.0),
        (spec.width_m + 0.8, foundation_height, spec.depth_m + 0.8),
        materials["dark_stone"],
        root,
        0.16,
    )
    wall_material = materials["plaster"] if spec.storeys == 1 else materials["stone"]
    _cube(
        "Cottage_%s_plaster" % spec.key,
        (0.0, foundation_height + spec.wall_height_m * 0.5, 0.0),
        (spec.width_m, spec.wall_height_m, spec.depth_m),
        wall_material,
        root,
        0.1,
    )
    roof_material = materials["slate"] if spec.roof_material == "slate" else materials["thatch"]
    _roof(
        "Cottage_%s_%s" % (spec.key, spec.roof_suffix),
        spec.width_m + 0.6,
        spec.depth_m + 0.6,
        foundation_height + spec.base_height_m,
        foundation_height + spec.ridge_height_m,
        roof_material,
        root,
    )

    wall_axis, sign, along_fraction = _DOOR_LOCAL_POSITIONS[spec.door_side]
    if wall_axis == "depth":
        door_x = along_fraction * spec.width_m
        door_z = sign * (spec.depth_m * 0.5 + 0.06)
        door_rotation = 0.0
    else:
        door_x = sign * (spec.width_m * 0.5 + 0.06)
        door_z = along_fraction * spec.depth_m
        door_rotation = math.pi * 0.5
    door_height = min(2.1, spec.wall_height_m * 0.68)
    door = _cube(
        "Cottage_%s_door" % spec.key,
        (door_x, foundation_height + door_height * 0.5, door_z),
        (1.05, door_height, 0.12),
        materials["timber"],
        root,
        0.05,
    )
    door.rotation_euler[2] = door_rotation

    # Windows sit on the front (depth-facing) wall regardless of door side --
    # the "side" door variant puts its door on a gable end, which never
    # shares a wall with these, so there is nothing to avoid colliding with.
    window_height = min(1.1, spec.wall_height_m * 0.28)
    window_y = foundation_height + spec.wall_height_m * 0.58
    for side in (-1.0, 1.0):
        _cube(
            "Cottage_%s_window_%d" % (spec.key, int(side)),
            (side * spec.width_m * 0.32, window_y, spec.depth_m * 0.5 + 0.04),
            (0.85, window_height, 0.14),
            materials["window"],
            root,
            0.03,
        )

    chimney_height = 1.6
    for order, (cx, cz) in enumerate(spec.chimney_offsets):
        suffix = "_chimney" if len(spec.chimney_offsets) == 1 else "_chimney_%d" % order
        _cube(
            "Cottage_%s%s" % (spec.key, suffix),
            (cx, foundation_height + spec.ridge_height_m + chimney_height * 0.5 - 0.3, cz),
            (0.6, chimney_height, 0.6),
            materials["dark_stone"],
            root,
            0.06,
        )
    return root


def _well(materials: dict[str, bpy.types.Material]) -> bpy.types.Object:
    root = bpy.data.objects.new("Well", None)
    bpy.context.collection.objects.link(root)
    _cylinder("Well_stone", 1.0, 1.1, (0.0, 0.55, 0.0), materials["stone"], root, 16)
    for side in (-1.0, 1.0):
        post = _cube(
            "Well_timber_%d" % int(side),
            (side * 0.85, 1.5, 0.0),
            (0.14, 1.8, 0.14),
            materials["timber"],
            root,
            0.02,
        )
        post.rotation_euler[0] = math.radians(side * 12.0)
    _cube("Well_roof", (0.0, 2.35, 0.0), (2.3, 0.14, 1.6), materials["thatch"], root, 0.06)
    return root


def _watchtower(materials: dict[str, bpy.types.Material]) -> bpy.types.Object:
    root = bpy.data.objects.new("Watchtower", None)
    bpy.context.collection.objects.link(root)
    _cylinder("Watchtower_stone", 2.4, 4.5, (0.0, 2.25, 0.0), materials["dark_stone"], root, 16)
    _cylinder("Watchtower_timber", 1.9, 8.5, (0.0, 4.5 + 4.25, 0.0), materials["timber"], root, 12)
    _cone("Watchtower_slate_roof", 2.6, 2.6, (0.0, 4.5 + 8.5 + 1.3, 0.0), materials["slate"], root, 12)
    for index in range(8):
        angle = math.tau * index / 8.0
        _cube(
            "Watchtower_merlon_%d" % index,
            (math.cos(angle) * 1.8, 4.5 + 8.5 + 0.4, math.sin(angle) * 1.8),
            (0.32, 0.8, 0.32),
            materials["dark_stone"],
            root,
            0.03,
        )
    return root


def _select_root(root: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    root.select_set(True)
    for child in root.children_recursive:
        child.select_set(True)
    bpy.context.view_layer.objects.active = root


def _export(root: bpy.types.Object, name: str, output: Path) -> None:
    _select_root(root)
    bpy.ops.export_scene.gltf(
        filepath=str(output / (name + ".glb")),
        export_format="GLB",
        use_selection=True,
        export_materials="EXPORT",
        export_normals=True,
        export_apply=True,
    )


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    materials = {
        "stone": _material("Cottage weathered granite", (0.12, 0.13, 0.11, 1.0), 0.94),
        "dark_stone": _material("Cottage granite shadow", (0.035, 0.043, 0.039, 1.0), 0.98),
        "plaster": _material("Cottage limewash", (0.62, 0.58, 0.48, 1.0), 0.88),
        "slate": _material("Cottage slate roof", (0.025, 0.038, 0.045, 1.0), 0.78),
        "thatch": _material("Cottage thatch roof", (0.30, 0.22, 0.09, 1.0), 0.95),
        "timber": _material("Cottage dark oak", (0.055, 0.027, 0.012, 1.0), 0.90),
        "window": _material(
            "Cottage warm window",
            (0.22, 0.11, 0.025, 1.0),
            0.55,
            emission=(0.55, 0.20, 0.035, 1.0),
        ),
    }

    generated: list[str] = []
    for spec in COTTAGE_VARIANTS:
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        root = _cottage(spec, materials)
        name = "highland_cottage_%s" % spec.key.lower()
        _export(root, name, output)
        generated.append(name)

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    root = _well(materials)
    _export(root, "highland_village_well", output)
    generated.append("highland_village_well")

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    root = _watchtower(materials)
    _export(root, "highland_watchtower", output)
    generated.append("highland_watchtower")

    bpy.ops.wm.save_as_mainfile(filepath=str(output / "highland_building_kit.blend"))
    print("Generated Highland building kit at %s: %s" % (output, ", ".join(generated)))


if __name__ == "__main__":
    main()
