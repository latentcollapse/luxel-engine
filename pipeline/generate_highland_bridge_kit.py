"""Generate a portable full-lane Highland stone bridge in Blender."""

from __future__ import annotations

import sys
from pathlib import Path

import bpy


def _arguments() -> Path:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise SystemExit(
            "Usage: blender --background --python "
            "generate_highland_bridge_kit.py -- OUTPUT_DIRECTORY"
        )
    return Path(args[0]).resolve()


def _material(
    name: str,
    color: tuple[float, float, float, float],
    roughness: float,
) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = color
    principled.inputs["Roughness"].default_value = roughness
    return material


def _cube(
    name: str,
    location: tuple[float, float, float],
    size: tuple[float, float, float],
    material: bpy.types.Material,
    root: bpy.types.Object,
    bevel: float = 0.0,
) -> bpy.types.Object:
    # Shared asset convention is X/Z ground with Y up; Blender is Z-up.
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


def _bridge(variant: int) -> bpy.types.Object:
    granite_tones = (
        (0.105, 0.125, 0.115, 1.0),
        (0.125, 0.118, 0.098, 1.0),
        (0.082, 0.105, 0.112, 1.0),
    )
    granite = _material(
        "Highland bridge granite", granite_tones[variant], 0.94
    )
    dark_stone = _material("Wet arch stone", (0.035, 0.052, 0.048, 1.0), 0.98)
    road = _material("Packed bridge road", (0.14, 0.105, 0.052, 1.0), 0.92)
    root = bpy.data.objects.new(
        "HighlandStoneBridge_%s" % "ABC"[variant], None
    )
    bpy.context.collection.objects.link(root)

    # Local +Z is the direction of travel. The 32 m deck matches Caledonia's
    # reviewed lane width; the 28 m span clears its widest 15 m stream.
    spans = (28.0, 32.0, 36.0)
    span = spans[variant]
    _cube("Stone_deck", (0.0, 0.75, 0.0), (32.0, 1.5, span), granite, root, 0.65)
    _cube("Packed_road_surface", (0.0, 1.58, 0.0), (27.0, 0.18, span + 0.2), road, root, 0.10)
    merlon_positions = (
        (-11.5, -5.75, 0.0, 5.75, 11.5),
        (-13.0, -8.0, -3.0, 3.0, 8.0, 13.0),
        (-15.0, -9.0, -3.0, 3.0, 9.0, 15.0),
    )[variant]
    parapet_height = (3.9, 3.25, 4.45)[variant]
    for side in (-1.0, 1.0):
        _cube(
            "Parapet_%s" % ("left" if side < 0 else "right"),
            (side * 15.1, 2.75, 0.0),
            (1.8, parapet_height, span),
            dark_stone,
            root,
            0.45,
        )
        for z in merlon_positions:
            _cube(
                "Parapet_merlon_%s_%s" % (side, z),
                (side * 15.1, 5.05, z),
                (2.4, 2.2, 2.3),
                granite,
                root,
                0.28,
            )

    # Two low piers leave a central water opening while keeping a readable
    # stone-arch silhouette from the player camera.
    pier_x = (-10.5, 10.5) if variant != 1 else (-8.5, 8.5)
    pier_z = (-span * 0.30, span * 0.30)
    for x in pier_x:
        for z in pier_z:
            _cube(
                "Buttress_%s_%s" % (x, z),
                (x, -1.1, z),
                (5.5, 3.7, 4.5),
                dark_stone,
                root,
                0.55,
            )
    for z in (-span * 0.48, span * 0.48):
        _cube("Approach_curb_%s" % z, (0.0, 0.4, z), (32.0, 0.8, 1.4), granite, root, 0.25)
    if variant == 1:
        # Low triangular-ish refuges create a broader old military crossing.
        for side in (-1.0, 1.0):
            _cube(
                "Watch_refuge_%s" % side,
                (side * 13.2, 2.2, 0.0),
                (4.8, 2.8, 7.0),
                granite,
                root,
                0.5,
            )
    elif variant == 2:
        # Tall end pylons distinguish the long mountain-channel variant.
        for side in (-1.0, 1.0):
            for z in (-span * 0.42, span * 0.42):
                _cube(
                    "End_pylon_%s_%s" % (side, z),
                    (side * 14.8, 5.1, z),
                    (3.4, 8.5, 3.4),
                    dark_stone,
                    root,
                    0.45,
                )
    return root


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for variant, suffix in enumerate(("a", "b", "c")):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        root = _bridge(variant)
        destination = output / ("highland_stone_bridge_" + suffix)
        bpy.ops.wm.save_as_mainfile(filepath=str(destination.with_suffix(".blend")))
        bpy.ops.object.select_all(action="DESELECT")
        root.select_set(True)
        for child in root.children_recursive:
            child.select_set(True)
        bpy.context.view_layer.objects.active = root
        bpy.ops.export_scene.gltf(
            filepath=str(destination.with_suffix(".glb")),
            export_format="GLB",
            use_selection=True,
            export_materials="EXPORT",
            export_normals=True,
            export_apply=True,
        )
        print("Generated Highland bridge variant at %s" % destination.with_suffix(".glb"))


if __name__ == "__main__":
    main()
