"""Generate six portable, textured granite cliff formations in Blender.

Run with:
    blender --background --python pipeline/generate_alpine_cliffs.py -- <output-dir>

The generator deliberately makes broad, layered rock masses with several
distinct summits rather than the single needle meshes that read as crystal
spikes at zone scale. It exports self-contained GLB assets plus a shared
granite/snow material source texture for inspection and future engine adapters.
"""

from __future__ import annotations

import math
import random
import sys
from pathlib import Path

import bpy


def _arguments() -> Path:
    if "--" not in sys.argv or len(sys.argv) != sys.argv.index("--") + 2:
        raise RuntimeError("Expected -- <output-dir>")
    return Path(sys.argv[-1]).resolve()


def _clean() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)


def _granite_texture(output_dir: Path) -> bpy.types.Image:
    size = 512
    image_path = output_dir / "highland_granite_albedo.png"
    if image_path.is_file():
        image = bpy.data.images.load(str(image_path), check_existing=False)
        return image
    random.seed(7719)
    image = bpy.data.images.new("highland_granite_albedo", width=size, height=size, alpha=False)
    pixels: list[float] = []
    for y in range(size):
        for x in range(size):
            broad = math.sin(x * 0.065 + y * 0.021) * 0.07 + math.cos(y * 0.045 - x * 0.018) * 0.05
            grain = random.uniform(-0.045, 0.045)
            warmth = broad + grain
            pixels.extend((0.22 + warmth, 0.245 + warmth * 0.9, 0.24 + warmth * 0.78, 1.0))
    image.pixels.foreach_set(pixels)
    image.filepath_raw = str(image_path)
    image.file_format = "PNG"
    image.save()
    return image


def _materials(texture: bpy.types.Image) -> tuple[bpy.types.Material, bpy.types.Material]:
    granite = bpy.data.materials.new("Highland Granite")
    granite.use_nodes = True
    nodes, links = granite.node_tree.nodes, granite.node_tree.links
    bsdf = nodes.get("Principled BSDF")
    bsdf.inputs["Roughness"].default_value = 0.88
    texture_node = nodes.new("ShaderNodeTexImage")
    texture_node.image = texture
    # Keep the export graph inside glTF's portable metallic/roughness subset.
    # The previous MixRGB tint looked correct in the authoring .blend but was
    # unsupported by the GLB exporter, so the imported asset silently became
    # white. The generated image is already authored in the intended dark,
    # wet-granite range and can bind directly to Base Color in every target.
    links.new(texture_node.outputs["Color"], bsdf.inputs["Base Color"])
    snow = bpy.data.materials.new("Wind-scoured Snow")
    snow.use_nodes = True
    snow_bsdf = snow.node_tree.nodes.get("Principled BSDF")
    snow_bsdf.inputs["Base Color"].default_value = (0.38, 0.43, 0.46, 1.0)
    snow_bsdf.inputs["Roughness"].default_value = 0.92
    return granite, snow


def _spire(name: str, rng: random.Random, center_x: float, center_z: float, radius_x: float, radius_z: float, height: float, granite: bpy.types.Material, snow: bpy.types.Material) -> bpy.types.Object:
    segments, rings = 16, 14
    vertices: list[tuple[float, float, float]] = []
    uvs: list[tuple[float, float]] = []
    faces: list[tuple[int, int, int]] = []
    for ring in range(rings):
        fraction = ring / float(rings - 1)
        taper = max(0.06, (1.0 - fraction) ** (0.58 + rng.uniform(-0.08, 0.08)))
        for segment in range(segments):
            angle = segment * math.tau / segments
            striation = 1.0 + 0.16 * math.sin(angle * 3.0 + rng.random() * 0.8) + 0.07 * math.sin(ring * 1.7 + angle * 5.0)
            x = center_x + math.cos(angle) * radius_x * taper * striation + rng.uniform(-0.8, 0.8) * (1.0 - fraction)
            z = center_z + math.sin(angle) * radius_z * taper * striation + rng.uniform(-0.8, 0.8) * (1.0 - fraction)
            y = fraction * height + 0.08 * height * math.sin(angle * 2.0 + fraction * 5.0) * fraction
            # The semantic terrain contract is X/Z ground with Y up. Blender
            # authors in X/Y ground with Z up; writing the engine tuple here
            # made the old exported formations lie across the ground after the
            # glTF coordinate conversion. Convert at the only mesh boundary so
            # Godot, Unity, and Unreal all receive an upright alpine silhouette.
            vertices.append((x, z, y))
            uvs.append((segment / float(segments), fraction))
    for ring in range(rings - 1):
        for segment in range(segments):
            current = ring * segments + segment
            next_segment = ring * segments + (segment + 1) % segments
            upper = (ring + 1) * segments + segment
            upper_next = (ring + 1) * segments + (segment + 1) % segments
            # Triangles preserve a hand-hewn faceted granite silhouette.
            faces.extend(((current, upper, upper_next), (current, upper_next, next_segment)))
    top_center = len(vertices)
    vertices.append((center_x + rng.uniform(-2.0, 2.0), center_z + rng.uniform(-2.0, 2.0), height * 1.07))
    uvs.append((0.5, 1.0))
    top_ring = (rings - 1) * segments
    for segment in range(segments):
        faces.append((top_ring + segment, top_center, top_ring + (segment + 1) % segments))
    mesh = bpy.data.meshes.new(name + "_mesh")
    mesh.from_pydata(vertices, [], faces)
    uv_layer = mesh.uv_layers.new(name="UVMap")
    for polygon in mesh.polygons:
        for loop_index in polygon.loop_indices:
            uv_layer.data[loop_index].uv = uvs[mesh.loops[loop_index].vertex_index]
    mesh.materials.append(granite)
    mesh.materials.append(snow)
    mesh.update()
    for polygon in mesh.polygons:
        average_height = sum(mesh.vertices[index].co.z for index in polygon.vertices) / len(polygon.vertices)
        # Snow is a wind-scoured summit accent, not a full white cap. The old
        # threshold turned most narrow high faces into a bright ice-crystal
        # field and hid the granite silhouette the semantic profile promised.
        polygon.material_index = 1 if average_height > height * 0.93 and rng.random() > 0.82 else 0
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def _formation(name: str, seed: int, granite: bpy.types.Material, snow: bpy.types.Material) -> bpy.types.Object:
    rng = random.Random(seed)
    parts: list[bpy.types.Object] = []
    summit_count = 3 + seed % 2
    for index in range(summit_count):
        angle = index * math.tau / summit_count + rng.uniform(-0.42, 0.42)
        distance = rng.uniform(14.0, 40.0)
        parts.append(_spire(
            name + "_summit_%d" % index, rng,
            math.cos(angle) * distance, math.sin(angle) * distance * 0.56,
            rng.uniform(28.0, 48.0), rng.uniform(18.0, 36.0), rng.uniform(115.0, 205.0), granite, snow,
        ))
    # Join the summits into one asset so placement has a readable massif-like
    # footprint rather than a scatter of independent needles.
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    formation = bpy.context.object
    formation.name = name
    for polygon in formation.data.polygons:
        polygon.use_smooth = False
    return formation


def _export(output_dir: Path, index: int, granite: bpy.types.Material, snow: bpy.types.Material) -> None:
    _clean()
    formation = _formation("alpine_granite_formation_%s" % chr(ord("a") + index), 5401 + index * 67, granite, snow)
    bpy.context.view_layer.objects.active = formation
    formation.select_set(True)
    basename = "alpine_granite_formation_%s" % chr(ord("a") + index)
    output = output_dir / (basename + ".glb")
    bpy.ops.export_scene.gltf(filepath=str(output), export_format="GLB", use_selection=True, export_materials="EXPORT", export_texcoords=True, export_normals=True, export_apply=True)
    unity_dir = output_dir / "unity"
    unity_dir.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.fbx(filepath=str(unity_dir / (basename + ".fbx")), use_selection=True, apply_scale_options="FBX_SCALE_ALL", path_mode="COPY", embed_textures=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(output_dir / ("alpine_granite_formation_%s.blend" % chr(ord("a") + index))))


def main() -> None:
    output_dir = _arguments()
    output_dir.mkdir(parents=True, exist_ok=True)
    texture = _granite_texture(output_dir)
    granite, snow = _materials(texture)
    for index in range(6):
        _export(output_dir, index, granite, snow)
    print("Generated six Codeweald alpine granite formations at %s" % output_dir)


if __name__ == "__main__":
    main()
