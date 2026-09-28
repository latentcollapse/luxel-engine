"""Deterministic Blender provider for a rigged character control GLB.

This module only invokes Blender and transports the generated artifact. Asset
identity, schema validation, rig/animation facts, and acceptance remain owned
by ``wge-asset-contract``.
"""

from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path
from typing import Sequence


_BLENDER_CONTROL_SCRIPT = r'''
import bpy
import math
import sys
from mathutils import Quaternion

output_path = sys.argv[sys.argv.index("--") + 1]
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

def material(name, color):
    value = bpy.data.materials.new(name)
    value.diffuse_color = (*color, 1.0)
    value.metallic = 0.0
    value.roughness = 0.72
    return value

materials = {
    "cloth": material("control_cloth", (0.12, 0.28, 0.42)),
    "skin": material("control_skin", (0.72, 0.43, 0.27)),
    "boots": material("control_boots", (0.09, 0.10, 0.12)),
}

# A compact, inspectable low-poly humanoid made from weighted, overlapping
# volume forms. The control exists to exercise real skin/animation export; it
# is not represented as production art quality.
parts = [
    ("pelvis", (0.0, 0.0, 0.98), (0.23, 0.15, 0.19), "root", "cloth"),
    ("torso", (0.0, 0.0, 1.28), (0.28, 0.17, 0.37), "spine", "cloth"),
    ("head", (0.0, 0.0, 1.72), (0.17, 0.15, 0.19), "spine", "skin"),
    ("left_upper_arm", (-0.34, 0.0, 1.38), (0.12, 0.13, 0.25), "spine", "cloth"),
    ("left_forearm", (-0.39, 0.0, 1.08), (0.10, 0.11, 0.22), "spine", "skin"),
    ("right_upper_arm", (0.34, 0.0, 1.38), (0.12, 0.13, 0.25), "spine", "cloth"),
    ("right_forearm", (0.41, 0.0, 1.10), (0.10, 0.11, 0.21), "hand_r", "skin"),
    ("left_leg", (-0.13, 0.0, 0.57), (0.13, 0.14, 0.43), "root", "cloth"),
    ("right_leg", (0.13, 0.0, 0.57), (0.13, 0.14, 0.43), "root", "cloth"),
    ("left_boot", (-0.13, -0.045, 0.12), (0.14, 0.22, 0.12), "root", "boots"),
    ("right_boot", (0.13, -0.045, 0.12), (0.14, 0.22, 0.12), "root", "boots"),
]

arm_data = bpy.data.armatures.new("control_skeleton")
rig = bpy.data.objects.new("control_skeleton", arm_data)
bpy.context.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
rig.select_set(True)
bpy.ops.object.mode_set(mode="EDIT")
root = arm_data.edit_bones.new("root")
root.head, root.tail = (0.0, 0.0, 0.0), (0.0, 0.0, 0.96)
spine = arm_data.edit_bones.new("spine")
spine.head, spine.tail, spine.parent = (0.0, 0.0, 0.96), (0.0, 0.0, 1.48), root
hand = arm_data.edit_bones.new("hand_r")
hand.head, hand.tail, hand.parent = (0.0, 0.0, 1.39), (0.48, 0.0, 1.10), spine
bpy.ops.object.mode_set(mode="OBJECT")
rig.select_set(False)

def make_lod(name, segments, rings):
    vertices = []
    faces = []
    polygon_materials = []
    group_vertices = {"root": [], "spine": [], "hand_r": []}
    for part_name, location, scale, bone_name, material_name in parts:
        base = len(vertices)
        vertices.append((location[0], location[1], location[2] + scale[2]))
        for ring in range(1, rings):
            latitude = math.pi * ring / rings
            for segment in range(segments):
                longitude = 2.0 * math.pi * segment / segments
                vertices.append((
                    location[0] + scale[0] * math.sin(latitude) * math.cos(longitude),
                    location[1] + scale[1] * math.sin(latitude) * math.sin(longitude),
                    location[2] + scale[2] * math.cos(latitude),
                ))
        bottom = len(vertices)
        vertices.append((location[0], location[1], location[2] - scale[2]))
        group_vertices[bone_name].extend(range(base, len(vertices)))
        material_index = list(materials).index(material_name)
        first_ring = base + 1
        for segment in range(segments):
            following = (segment + 1) % segments
            faces.append((base, first_ring + segment, first_ring + following))
            polygon_materials.append(material_index)
        for ring in range(rings - 2):
            upper = first_ring + ring * segments
            lower = upper + segments
            for segment in range(segments):
                following = (segment + 1) % segments
                faces.append((upper + segment, lower + segment, lower + following, upper + following))
                polygon_materials.append(material_index)
        last_ring = first_ring + (rings - 2) * segments
        for segment in range(segments):
            following = (segment + 1) % segments
            faces.append((last_ring + segment, bottom, last_ring + following))
            polygon_materials.append(material_index)

    mesh_data = bpy.data.meshes.new(name)
    mesh_data.from_pydata(vertices, [], faces)
    mesh_data.update()
    obj = bpy.data.objects.new(name, mesh_data)
    bpy.context.collection.objects.link(obj)
    for value in materials.values():
        mesh_data.materials.append(value)
    for polygon, material_index in zip(mesh_data.polygons, polygon_materials):
        polygon.material_index = material_index
    for group_name, indices in group_vertices.items():
        group = obj.vertex_groups.new(name=group_name)
        group.add(indices, 1.0, "REPLACE")
    mesh = obj
    mesh.name = name
    mesh.data.name = name
    modifier = mesh.modifiers.new("skin", "ARMATURE")
    modifier.object = rig
    mesh.parent = rig
    return mesh

lod0 = make_lod("body_lod0", 12, 8)
lod1 = make_lod("body_lod1", 8, 5)

socket = bpy.data.objects.new("weapon_mount", None)
bpy.context.collection.objects.link(socket)
socket.parent = rig
socket.parent_type = "BONE"
socket.parent_bone = "hand_r"
socket.location = (0.10, 0.0, 0.0)
socket["wge_socket"] = True

def make_action(name, frames):
    action = bpy.data.actions.new(name)
    rig.animation_data_create()
    rig.animation_data.action = action
    pose = rig.pose.bones["spine"]
    wrist = rig.pose.bones["hand_r"]
    for frame, spine_angle, wrist_angle, root_lift in frames:
        pose.rotation_mode = "QUATERNION"
        pose.rotation_quaternion = Quaternion((1.0, 0.0, 0.0), spine_angle)
        pose.keyframe_insert(data_path="rotation_quaternion", frame=frame, group="spine")
        wrist.rotation_mode = "QUATERNION"
        wrist.rotation_quaternion = Quaternion((0.0, 1.0, 0.0), wrist_angle)
        wrist.keyframe_insert(data_path="rotation_quaternion", frame=frame, group="hand_r")
        rig.pose.bones["root"].location = (0.0, 0.0, root_lift)
        rig.pose.bones["root"].keyframe_insert(data_path="location", frame=frame, group="root")
    track = rig.animation_data.nla_tracks.new()
    track.name = name
    strip = track.strips.new(name, 1, action)
    strip.action_frame_start = 1
    strip.action_frame_end = 25
    strip.frame_start = 1
    strip.blend_type = "REPLACE"

make_action("idle", [(1, -0.10, 0.0, 0.0), (13, 0.10, 0.02, 0.01), (25, -0.10, 0.0, 0.0)])
make_action("locomotion", [(1, -0.08, 0.0, 0.0), (13, 0.08, 0.0, 0.035), (25, -0.08, 0.0, 0.0)])
make_action("attack", [(1, 0.0, -0.65, 0.0), (13, -0.12, 0.72, 0.0), (25, 0.0, -0.65, 0.0)])

bpy.context.scene.render.fps = 24
bpy.context.scene.frame_start = 1
bpy.context.scene.frame_end = 25
bpy.ops.object.select_all(action="SELECT")
bpy.context.view_layer.objects.active = rig
bpy.ops.export_scene.gltf(
    filepath=output_path,
    export_format="GLB",
    export_animations=True,
    export_animation_mode="NLA_TRACKS",
    export_skins=True,
    export_extras=True,
    export_yup=True,
    export_apply=True,
    export_cameras=False,
    export_lights=False,
)
'''


def generate_rigged_character_control(
    output_glb: str | os.PathLike[str],
    *,
    blender_executable: str | os.PathLike[str] | None = None,
    timeout_seconds: float = 120.0,
) -> subprocess.CompletedProcess[str]:
    """Generate a deterministic, inspectable rigged character GLB using Blender."""

    blender = blender_executable or os.environ.get("WGE_BLENDER_BIN", "blender")
    destination = Path(output_glb).resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="wge-rigging-provider-") as temporary:
        script = Path(temporary) / "generate_control.py"
        script.write_text(_BLENDER_CONTROL_SCRIPT, encoding="utf-8")
        command: Sequence[str] = (
            str(blender),
            "--background",
            "--factory-startup",
            "--python",
            str(script),
            "--",
            str(destination),
        )
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout_seconds,
        )
    if result.returncode == 0 and not destination.is_file():
        raise RuntimeError("Blender reported success but did not produce the requested GLB")
    return result
