"""Generate portable highland conifer families with explicit LOD variants.

Run with Blender:
    blender --background --python pipeline/generate_highland_foliage.py -- <output-dir>

The asset planner resolves ``lod0`` selections into complete observed
``lod0``/``lod1``/``lod2`` families. Engine adapters consume those siblings
rather than asking an image model to invent inconsistent tree meshes.
"""

from __future__ import annotations

import json
import math
import random
import sys
from pathlib import Path

import bpy


FOLIAGE_VERSION = "codeweald.highland-foliage/v1"
SPECIES = (
    # glTF imports the Blender material values as linear base colour. These
    # intentionally low values become deep Highland greens after engine colour
    # conversion, rather than the accidental mint canopy of the first pass.
    ("highland_pine", 7101, 16.0, 4.5, (0.006, 0.025, 0.010, 1.0)),
    ("windswept_spruce", 7207, 20.0, 5.6, (0.004, 0.018, 0.007, 1.0)),
    ("mountain_fir", 7319, 13.5, 4.0, (0.009, 0.032, 0.013, 1.0)),
)
LOD_TIERS = ((0, 8, 12), (1, 5, 8), (2, 2, 6))  # level, branch layers, radial sides


def _arguments() -> Path:
    if "--" not in sys.argv or len(sys.argv) != sys.argv.index("--") + 2:
        raise RuntimeError("Expected -- <output-dir>")
    return Path(sys.argv[-1]).resolve()


def _clean() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablock in list(bpy.data.materials):
        bpy.data.materials.remove(datablock)


def _material(name: str, color: tuple[float, float, float, float], roughness: float) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = color
    principled.inputs["Roughness"].default_value = roughness
    return material


def _cone(name: str, radius: float, depth: float, location: tuple[float, float, float], sides: int, material: bpy.types.Material, skew_x: float = 0.0) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cone_add(vertices=sides, radius1=max(radius, 0.02), radius2=0.0, depth=max(depth, 0.04), location=location)
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(material)
    # Slight lean and nonuniform width stops the profile reading as a stack of
    # mathematically identical cones while retaining a cheap clean silhouette.
    obj.rotation_euler = (0.0, skew_x, 0.0)
    obj.scale.x = 0.86 + abs(math.sin(skew_x)) * 0.18
    obj.scale.y = 1.0
    return obj


def _needle_clump(name: str, location: tuple[float, float, float], radius: float, material: bpy.types.Material, angle: float) -> bpy.types.Object:
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=max(radius, 0.05), location=location)
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(material)
    obj.scale = (0.78, 0.78, 0.42)
    obj.rotation_euler = (0.0, angle, 0.0)
    return obj


def _tree(name: str, seed: int, height: float, crown_radius: float, needle_color: tuple[float, float, float, float], lod: int, layers: int, sides: int) -> list[bpy.types.Object]:
    rng = random.Random(seed + lod * 313)
    bark = _material(name + "_bark", (0.11, 0.065, 0.035, 1.0), 0.96)
    needles = _material(name + "_needles", needle_color, 0.88)
    objects: list[bpy.types.Object] = []
    lean = rng.uniform(-0.16, 0.16) if "windswept" in name else rng.uniform(-0.055, 0.055)
    # Blender is Z-up. The glTF exporter converts this to the engines' Y-up
    # convention; authoring height in the middle coordinate would export a
    # sideways tree and produce deceptively plausible thumbnails.
    trunk = _cone(name + "_trunk", max(0.24, crown_radius * 0.085), height * 0.95, (lean * height * 0.20, 0.0, height * 0.475), max(5, sides), bark, lean * 0.25)
    trunk.data.materials.clear()
    trunk.data.materials.append(bark)
    objects.append(trunk)
    crown_base = height * 0.16
    for index in range(layers):
        fraction = index / float(max(layers - 1, 1))
        center_y = crown_base + fraction * height * 0.70
        taper = 1.0 - fraction * 0.83
        radius = crown_radius * taper * rng.uniform(0.86, 1.10)
        cone_height = height * (0.34 - fraction * 0.018) * rng.uniform(0.86, 1.10)
        center_x = lean * center_y * (0.35 + fraction * 0.40)
        crown = _cone(name + "_crown_%02d" % index, radius, cone_height, (center_x, rng.uniform(-0.18, 0.18), center_y), sides, needles, rng.uniform(-0.18, 0.18))
        objects.append(crown)
        if lod == 0 and index % 2 == 0:
            # Separate clustered needles make the close silhouette read as a
            # tree family rather than a single procedural traffic cone, while
            # keeping the distant LODs deliberately cheap.
            for branch in range(4):
                angle = float(branch) * math.tau / 4.0 + rng.uniform(-0.20, 0.20)
                reach = radius * rng.uniform(0.34, 0.72)
                objects.append(_needle_clump(
                    name + "_needles_%02d_%d" % (index, branch),
                    (center_x + math.cos(angle) * reach, math.sin(angle) * reach, center_y + rng.uniform(-cone_height * 0.10, cone_height * 0.10)),
                    max(0.26, radius * rng.uniform(0.24, 0.37)), needles, angle,
                ))
    return objects


def _export(output: Path, species: str, seed: int, height: float, radius: float, color: tuple[float, float, float, float], lod: int, layers: int, sides: int) -> dict[str, object]:
    _clean()
    basename = "%s_lod%d" % (species, lod)
    objects = _tree(basename, seed, height, radius, color, lod, layers, sides)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    # A family is one runtime prop, not 20+ tiny scene nodes. Joining preserves
    # the three shared material slots while keeping the explicit LOD boundary
    # at the exported-asset level.
    bpy.ops.object.join()
    glb = output / (basename + ".glb")
    bpy.ops.export_scene.gltf(filepath=str(glb), export_format="GLB", use_selection=True, export_materials="EXPORT", export_normals=True, export_apply=True)
    unity = output / "unity"
    unity.mkdir(parents=True, exist_ok=True)
    fbx = unity / (basename + ".fbx")
    bpy.ops.export_scene.fbx(filepath=str(fbx), use_selection=True, apply_scale_options="FBX_SCALE_ALL", path_mode="COPY", embed_textures=True)
    return {"lod": lod, "godot_glb": glb.name, "unity_fbx": (Path("unity") / fbx.name).as_posix(), "triangle_budget": {0: 1800, 1: 600, 2: 140}[lod]}


def main() -> None:
    output = _arguments()
    output.mkdir(parents=True, exist_ok=True)
    families = []
    for species, seed, height, radius, color in SPECIES:
        variants = [_export(output, species, seed, height, radius, color, lod, layers, sides) for lod, layers, sides in LOD_TIERS]
        families.append({"id": species, "variants": variants})
    (output / "foliage_lod_manifest.json").write_text(json.dumps({"schema_version": FOLIAGE_VERSION, "families": families}, indent=2) + "\n", encoding="utf-8")
    print("Generated %d highland foliage families at %s" % (len(families), output))


if __name__ == "__main__":
    main()
